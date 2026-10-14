#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Real, privileged BPF integration gate. Never substitutes mocks for the kernel."""

import argparse
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]


def snapshot():
    return {
        kind: json.loads(subprocess.check_output(["bpftool", "-j", kind, "show"]))
        for kind in ("prog", "map", "link")
    }


def added(before, after):
    return {
        kind: sorted({obj["id"] for obj in after[kind]} - {obj["id"] for obj in before[kind]})
        for kind in before
    }


def clean_after(before):
    # Kernel BPF object destruction may finish after an RCU grace period.
    # bpftool itself loads temporary iterator maps while enumerating. Compare
    # identities across two scans, not names: its maps disappear between scans;
    # a genuine residual profiler map retains its ID and must fail the gate.
    deadline = time.monotonic() + 5
    previous = snapshot()
    while True:
        after = snapshot()
        first = added(before, previous)
        second = added(before, after)
        leaked = {kind: sorted(set(first[kind]) & set(second[kind])) for kind in before}
        if not any(leaked.values()):
            return {"first": previous, "second": after, "persistent_new_ids": leaked}
        if time.monotonic() >= deadline:
            raise AssertionError(f"residual BPF objects: {leaked}")
        previous = after
        time.sleep(0.1)


def stop(process):
    if process is not None and process.poll() is None:
        process.kill()
        process.wait(timeout=5)


def run_case(binary, fixture, work, mode):
    before = snapshot()
    target = subprocess.Popen([str(fixture)])
    capture = None
    log_path = work / f"{mode}.log"
    try:
        with log_path.open("w") as log:
            capture = subprocess.Popen([
                str(binary), "probe-test", "--pid", str(target.pid),
                "--binary", str(fixture), "--symbol",
                "no_such_symbol" if mode == "attach-error" else "vp_test_frame",
                "--ttl", "3" if mode == "ttl" else "15",
            ], stdout=log, stderr=subprocess.STDOUT)
            during = None
            if mode != "attach-error":
                deadline = time.monotonic() + 8
                while True:
                    text = log_path.read_text()
                    if re.search(r"hits=[1-9][0-9]*", text):
                        break
                    if capture.poll() is not None or time.monotonic() >= deadline:
                        raise AssertionError(f"{mode}: no real uprobe hits:\n{text}")
                    time.sleep(0.05)
                during = snapshot()
                new = added(before, during)
                assert new["prog"] and new["map"], f"no live BPF objects: {new}"
                assert any(p.get("name") == "vp_probe" and p["id"] in new["prog"]
                           for p in during["prog"]), "test program not visible"
                fds = Path(f"/proc/{capture.pid}/fd")
                assert not any(os.readlink(fd).startswith("socket:") for fd in fds.iterdir()), \
                    "step 1 capture unexpectedly opened a socket"
                if mode == "target-exit":
                    target.terminate()
                    target.wait(timeout=5)
                elif mode in ("sigint", "sigterm", "sigkill"):
                    capture.send_signal({"sigint": signal.SIGINT, "sigterm": signal.SIGTERM,
                                         "sigkill": signal.SIGKILL}[mode])
            status = capture.wait(timeout=20)
        text = log_path.read_text()
        if mode == "attach-error":
            assert status != 0 and "attach uprobe" in text, text
        elif mode == "sigkill":
            assert status == -signal.SIGKILL, text
        else:
            reason = {"sigint": "signal", "sigterm": "signal"}.get(mode, mode)
            assert status == 0 and f"detached reason={reason}" in text, text
            if mode == "ttl":
                assert len(re.findall(r"^hits=", text, re.MULTILINE)) <= 3, \
                    f"more than 1Hz counter reads near TTL expiry:\n{text}"
        assert not Path(f"/proc/{capture.pid}").exists(), "capture process still present"
        after = clean_after(before)
        return {"case": mode, "status": "passed", "before": before,
                "during": during, "after": after, "output": text}
    finally:
        stop(capture)
        stop(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/vapour-profiler")
    parser.add_argument("--fixture", type=Path, default=ROOT / "target/probe-target")
    parser.add_argument("--report", type=Path, default=ROOT / "target/bpf-lifecycle.json")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    fixture = args.fixture.resolve(strict=True)
    args.report.unlink(missing_ok=True)
    before = snapshot()  # Fail loudly if bpftool or inspection privileges are absent.
    # Help and invalid arguments must not load anything, even when privileged.
    subprocess.run([str(binary), "--help"], check=True, stdout=subprocess.DEVNULL)
    for pid, ttl in (("0", "1"), ("-1", "1"), ("1", "0"), ("1", "301")):
        invalid = subprocess.run([str(binary), "probe-test", f"--pid={pid}",
                                  "--binary", str(fixture), "--symbol", "vp_test_frame",
                                  "--ttl", ttl], capture_output=True)
        assert invalid.returncode != 0, "invalid target or TTL accepted"
    clean_after(before)
    results = []
    with tempfile.TemporaryDirectory(prefix="vp-lifecycle-", dir=ROOT / "target") as tmp:
        for mode in ("ttl", "sigint", "sigterm", "target-exit", "sigkill", "attach-error"):
            results.append(run_case(binary, fixture, Path(tmp), mode))
            print(f"PASS {mode}: no residual program/map/link IDs", flush=True)
    args.report.write_text(json.dumps({"kernel": os.uname().release, "cases": results}, indent=2) + "\n")
    print(f"Evidence: {args.report}")


if __name__ == "__main__":
    main()
