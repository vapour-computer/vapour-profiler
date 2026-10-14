<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Local performance dashboard

Start explicitly; no service, download or listener runs beforehand:

```sh
cargo build --locked --workspace
target/debug/vapour-profiler dashboard
# Optional: authorize one local target, then press Start native capture in the UI.
target/debug/vapour-profiler dashboard --pid YOUR_PID --ttl 300
```

Two opt-in launch flags (both require `--pid`):

- `--tree` sums CPU and memory over the PID and all its child processes. Use it for browsers, whose game
  work runs in renderer and GPU child processes. Only the root PID's exit ends the session.
- `--autostart` starts native capture immediately instead of waiting for **Start native capture**. Capture is
  still explicit: it begins only because the launching command passed the flag (`vapour test` does).

Open the full URL printed in the terminal in **Chromium**. Port defaults to
`127.0.0.1:6767`; `--port` changes it. The token is in the URL fragment and is
removed from browser history immediately. It stays only in page memory. Reloading
requires the original launch URL. A port conflict fails without killing its owner.

## Implemented

- Embedded HTML/CSS and vanilla JS; no UI framework, CDN or external fonts.
- Shared-window Canvas timelines with spike-preserving drawing buckets and cursor.
- Searchable/click-to-zoom sample-weighted flamegraph, up to 4096 nodes/64 levels.
  It uses real folded stacks; flat samples get no invented callers.
- Schema-validated `.vpt` imports and `.folded` imports. 50MiB file, 1MiB record,
  150,000-record limits. No probes are started by import.
- Native process CPU, RSS and permission-dependent PSS via `/proc`, read at 1Hz
  **only after clicking Start**. Target PID comes exclusively from the local CLI.
- Local trace export, last-20-actions log, spike and grow-event lists.
- Deterministic offline reports, direct Claude summaries, grounded evidence IDs,
  hotspot rankings, memory-growth warnings, JSON report export and summary comparison.
- OS keyring through `secret-tool`/Secret Service, or `ANTHROPIC_API_KEY` from the
  launching shell. Install libsecret's command-line tool for keyring operations.
  The UI clears the key field immediately and never stores it in browser storage.

## Scope and interpretation

Native FPS, native BPF CPU-stack sampling, off-CPU probes and runtime-specific
adapters are not yet integrated into this dashboard. Those panels require real
imported trace data and otherwise show unavailable. Browser traces can be imported;
automatic player forwarding, independent simultaneous sources and Vapour CLI
orchestration are not connected yet. Browser collection remains independent.

Charts support a selected time window. Flamegraphs and analysis explicitly use
the entire retained trace; they do not claim time-window stack attribution.
Compare the same game/workload/options. Memory growth alone is not proof of a leak.
Optimization suggestions do not edit or execute anything in a game repository.

## Lifecycle and budgets

The explicit dashboard/viewer session has a maximum/default 300-second TTL.
Opening the viewer keeps its local API available but starts no target collection.
End session, Ctrl-C, SIGTERM, target exit or TTL closes the agent and its sockets.
There is no auto-reconnect polling. Native captures are flushed to
`$XDG_DATA_HOME/vapour-profiler/dumps`, defaulting to
`~/.local/share/vapour-profiler/dumps`, with mode 0600. Storage is capped at 200MiB;
existing user dumps are never overwritten or removed. Export before ending if
storage is full. Imported dumps stay in memory and are not automatically saved.

Four WebSocket clients, eight in-flight HTTP handlers, one import/analysis at a
time, 30-second request deadline, two Tokio workers, 300 native metric buckets.
The three UI assets total under 40KiB uncompressed. Imported trace parsing and
explicit analysis can allocate more; file/record limits bound those operations.

## Local API v1 (initial dashboard API)

Every `/api/*` request requires `Authorization: Bearer SESSION_TOKEN`. WebSockets
use protocols `vapour.v1` and the token; the server selects `vapour.v1`. Host must
be the exact loopback host/port. If an Origin is present, only this dashboard's
same origin is accepted. There is no remote/Vapour-origin allowlist in this version.
No CORS wildcard or private-network access header is sent.

| Endpoint | Operation |
| --- | --- |
| `GET /api/status` | Session/capabilities and last metric |
| `GET /api/live` | Authenticated WS; status/metric update at most 1Hz |
| `POST /api/start` | Starts collection for the CLI-authorized PID only |
| `POST /api/trace` | Raw UTF-8 `.vpt`; validated before replacing inactive view |
| `GET /api/trace` | Current retained trace; active capture export is partial |
| `GET /api/assets` | Per-asset cost report (ranked by hardness) from the retained trace's `asset` records; empty without them |
| `POST /api/analyze` | `{offline: true}` or `{offline: false, model: "claude-sonnet-4-6"}` |
| `POST /api/key` | `{key: "..."}`; stores in OS keyring, never echoes |
| `POST /api/key/remove` | Removes keyring entry; env key must be unset in the shell |
| `POST /api/off` | Flush native capture and shut down |

The only remote request is an explicit analysis to Anthropic, carrying a bounded
summary (24KB cap), never raw traces. Responses have a 256KiB cap and 20-second
deadline. Session shutdown cancels in-flight provider requests. Unsupported model
findings are dropped; errors return an offline report. Live provider requests and
OS keyring access need your own environment; tests use no real key.

This standalone local UI does not ship in any Vapour production web bundle.
Staging dashboard/player wiring still needs its independent backend/owner guards.

## Verification

```sh
cargo test -p vp-analyze
cargo clippy --locked --workspace --all-targets -- -D warnings
npm ci --prefix tests/dashboard
CHROMIUM_PATH=/usr/bin/chromium node tests/dashboard/smoke.mjs
```

The real Chromium test checks token/origin/Host rejection, dump import/export,
flamegraph and report rendering, hotspot ranking/growth warnings, live process
metrics, native dump flush, and off/TTL socket cleanup. Key saving is intercepted;
no existing keyring secret or real provider account is touched. Screenshot:
`target/dashboard.png`. Firefox verification and overhead benchmarks are pending.
