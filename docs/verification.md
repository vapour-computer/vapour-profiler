<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Step 1 verification — 2026-10-14

Host: Linux `7.2.9-arch1-1`, x86_64, kernel BTF present. Rust 1.99.0.

## Passed on the current code

- `cargo fmt --all --check`
- `cargo build --locked --workspace`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- C fixture compilation with `-Wall -Wextra -Werror`.
- CLI help works without privileges or an active capture.

Host clang/bpftool were absent from this execution environment. For the build,
official Arch clang 23.1.1 and bpf 7.2.10 packages were downloaded, verified with
`pacman-key --verify`, and extracted under `/tmp/opencode/vp-toolchain`.
No system package installation or permanent capability change was made.

Build environment used locally:

```sh
export CLANG=/tmp/opencode/vp-toolchain/usr/bin/clang
export LD_LIBRARY_PATH=/tmp/opencode/vp-toolchain/usr/lib
export TMPDIR=/tmp/opencode
cargo build --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

## Kernel proof and remaining gate

An explicitly authorized `pkexec` run passed all six real kernel cases: TTL,
SIGINT, SIGTERM, target exit, SIGKILL and attachment failure. Positive uprobe
hits were observed; no persistent new program, map or link IDs remained.
Captures had no socket descriptors and their processes exited.
Raw evidence from that run is in `target/bpf-lifecycle.json` (local, ignored).

That run exposed duplicate reads within the final fractional millisecond of
TTL: truncating a poll timeout to integer milliseconds caused a brief spin.
The current code rounds waits up and enforces a separate 1Hz read deadline.
The test now rejects excessive reads in the three-second TTL case.

**The corrected code has not yet passed the privileged kernel gate.** Two
subsequent authorization attempts were denied before the harness could run.
The earlier report therefore documents the prior code, not final acceptance.
Step 1's runtime gate remains pending until the following succeeds:

```sh
sudo env PATH=/tmp/opencode/vp-toolchain/usr/bin:/usr/bin \
  python3 /home/vroomer/projects/vapour-profiler/tests/bpf_lifecycle.py
```

With system bpftool installed, `sudo python3 tests/bpf_lifecycle.py` from the
repo is sufficient. No overhead budget or Linux-5.15 compatibility measurement
is claimed by these tests; those require their later workload/platform gates.
