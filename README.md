<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Vapour Profiler

Standalone, local-first eBPF performance observability for Wasm runtimes and
Linux processes. No Vapour CLI, account, SDK, or service is required.

**Status: local dashboard + embedded CO-RE BPF attach/detach proof.** This is not yet the
complete profiler. Native stack collection, runtime adapters, TUI and
release distribution follow the [build plan](docs/build-plan.md). The workspace
is deliberately unpublished until those acceptance gates pass.
See the [verification record](docs/verification.md) for checks and pending gates.
The local dashboard is available at `127.0.0.1:6767` after explicitly running
`target/debug/vapour-profiler dashboard [--pid PID]`. Open the full token-bearing
URL printed by the command. See [dashboard usage and current capabilities](docs/dashboard.md).
Vapour CLI synchronization and automatic browser forwarding remain pending.

## Invariants

- Capture is explicit. Off means no resident process, BPF objects, polling,
  socket, download, or analysis client.
- Local capture, storage, and analysis; no Vapour server traffic.
- Bounded collection, Lite by default once profiles are implemented. Heavy
  probes are opt-in. Overhead targets are goals until measured on real workloads.
- Runtime support comes from optional adapters, never a Vapour dependency.
- Future Vapour dashboard and browser integrations live in their own repos,
  staging-only. Browser telemetry shares only the versioned trace specification.

## Build prerequisites

Initial target: Linux 5.15+, x86_64, with `/sys/kernel/btf/vmlinux`. Build as your
normal user using stable Rust, a C compiler, clang with the BPF target,
pkg-config, Linux UAPI headers, libelf and zlib development files.
`libbpf-rs` vendors libbpf; libelf/zlib are currently system-linked.

Arch Linux:

```sh
sudo pacman -S --needed base-devel clang libelf zlib bpf
```

Debian/Ubuntu (bpftool's package varies by distribution):

```sh
sudo apt install build-essential clang pkg-config libelf-dev zlib1g-dev linux-libc-dev bpftool
```

```sh
cargo build --locked --workspace
cc -O0 -g -Wall -Wextra -Werror -o target/probe-target tests/fixtures/probe-target.c
target/debug/vapour-profiler --help
```

`CLANG=/path/to/clang` selects a compiler. `libbpf-cargo` builds the C program and
generates a skeleton in Cargo's output directory. The BPF ELF is embedded in
the binary; capture never compiles or downloads BPF. The probe uses a minimal
CO-RE view of `task_struct.tgid`, relocated against the running kernel's BTF.

## Step 1 attach/detach proof

The standalone BPF proof uses the development `probe-test` command:

```sh
target/probe-target &
game_pid=$!
target/debug/vapour-profiler probe-test \
  --pid "$game_pid" --binary "$PWD/target/probe-target" \
  --symbol vp_test_frame --ttl 10
kill "$game_pid"
```

The attach command needs session BPF/perf privileges; without them it fails
with a contextual error. Per-session `pkexec`/capability setup is step 2. Do not
make the test binary setuid. No root login or privileged build is needed.
The test CLI accepts only a positive PID and its actual executable. It is a
local developer command, not an interface for dashboard/URL-supplied inputs.

One per-CPU counter entry and read-only PID configuration are allocated; no
ring buffer, event stream, pins, or socket exists. Userspace reads the counter
at most once per second. TTL is 30 seconds by default, hard-capped at 300.
Ctrl-C, SIGTERM, target exit (pidfd), or TTL release the link before the skeleton.
Load/attach/output errors unwind the same resources. SIGKILL relies on kernel
descriptor cleanup. The target's executable inode is held open during capture.

## Verify the actual kernel lifecycle

After the unprivileged build above, explicitly authorize the focused test:

```sh
sudo python3 tests/bpf_lifecycle.py
```

This test-only harness starts its own fixture targets and bounded captures;
production session privilege handling is a separate step. Run on a quiet
host: the test rejects any newly surviving BPF IDs, including concurrent ones
created by other tools. Two consecutive after-scans distinguish bpftool's own
temporary iterator maps from persistent object IDs; both scans are recorded.
It records `bpftool -j prog/map/link show` snapshots
before, during, and after each case, confirms positive probe hits and absence
of sockets, and checks process exit and cleanup for:

- TTL, SIGINT, SIGTERM, target exit;
- SIGKILL (kernel closes every descriptor);
- symbol lookup/attachment failure after BPF loading;
- help and invalid PID/TTL inputs (no BPF load).

Evidence goes to `target/bpf-lifecycle.json`. Missing tools/privileges or a
kernel restriction is a failure, not a skipped/passing integration test.
No overhead numbers are claimed by this lifecycle test.

Developer checks:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

## Workspace and licensing

| Crate | Responsibility | Current state |
| --- | --- | --- |
| `vp-bpf` | C probes, CO-RE, embedded skeleton | Test uprobe |
| `vp-agent` | Loading, lifecycle, collection, local API | BPF proof + local dashboard/process metrics |
| `vp-adapters` | Runtime hook descriptions, JIT sources | Reserved |
| `vp-analyze` | Deterministic summary, provider trait, validated report | Trace validation, offline/Claude reports |
| `vp-cli` (`vapour-profiler`) | Standalone CLI and TUI | `probe-test`, `dashboard` |

Userspace: [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
BPF C: [GPL-2.0-only](LICENSE-GPL-2.0). See [LICENSE](LICENSE).
