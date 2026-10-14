<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Implementation gates

Implement in this order. A gate is complete only with recorded verification.

1. **Repository/BPF pipeline:** five-crate workspace, licenses/SPDX, embedded
   libbpf skeleton, real uprobe hits, clean `bpftool` inventory after exit.
2. **Toggle lifecycle:** `on --pid|--exec`, `off`, TTL (default 300s), target
   exit/signals, partial flush, per-session privileges. No service while off.
3. **Lite:** generic/wasmtime adapters, TOML custom adapters, optional frame
   marker, CPU and RSS/PSS at 1Hz, memory.grow where supported, 49Hz stack
   sampling, perfmap/jitdump symbolization, ratatui TUI. Unsupported data hidden.
4. **Trace contract:** versioned JSONL `.vpt`, JSON Schema, metadata, 1Hz series,
   frame timeline, spike windows, folded stacks and optional probe statistics;
   50MB dump cap, folded export. Accept conforming third-party traces.
5. **Analysis:** deterministic bounded summary, offline markdown/JSON report,
   provider trait and Claude client, <~8k-token summaries, evidence validation,
   keyring/env key handling, explicit network errors with offline fallback.
6. **Local API/full probes:** loopback agent on `127.0.0.1:6767` (updated
   requirement; replaces random default port), session authentication,
   origin allowlist (empty default), allowlisted-only private-network headers,
   versioned API docs. Opt-in sched/syscalls/faults/imports/alloc, 99Hz Full,
   in-kernel aggregation, 4MB ring cap/drop counts, measured overhead governor.
7. **Vapour adapter/releases:** obtain runtime symbols first; optional built-in
   adapter only. Signed GitHub releases/checksums, changelog, publish crates
   for `cargo install vapour-profiler`; document measured overhead and perms.
8. **Go CLI repo:** synchronized agent lifecycle and telemetry on port 6767;
   pinned lazy verified installation/rollback/removal, local
   game-ID resolution, scope commands, strict `vapour://scope/on|off`, native
   first-origin consent, hardcoded staging callback/origin. No remote raw PIDs.
9. **Environment guardrails before UI:** obtain existing configuration first.
   Separate Firebase projects; production config cannot enable devtools;
   production routes 404; prefs denied in production and game-owner-only in
   staging; build exclusion plus bundle, rules-emulator and deploy tests.
10. **Staging dashboard:** backend-enabled runtime guard, local token-authenticated
    stream, lifecycle/error states, synced graphs, drill-down, dump/analyze,
    keyring flow with immediate input clearing. Preferences only in Firestore.
11. **Browser package:** obtain player memory export first. Independent TypeScript
    implementation pinned to `.vpt` spec; staging + owner + explicit-toggle
    gated dynamic import; feature-detected metrics, bounded rings, opt-in
    self-profiling, BroadcastChannel and overlay, local key storage and direct
    Claude analysis. Off/TTL disposes hooks/observers/buffers, inert until reload.
12. **Demo/end-to-end:** planted slow function and leak under standalone wasmtime,
    native Vapour and browser. Validate shared traces, rank the slow function
    first, flag leak, prove overhead budgets and production exclusion.

## Fixed constraints

- Off costs zero resident resources. No startup install, background polling,
  API socket, key client, or telemetry-module fetch.
- Performance targets (not yet measurements): FPS/CPU/memory <0.5%, Lite <1%,
  Full <3%. Every capture/buffer has a hard bound; heavy probes are opt-in.
- Captures, traces, summaries and reports remain local. Optional Claude
  requests go directly to the provider with a deterministic summary only.
  Keys never go through Vapour servers, Firestore, analytics, URLs or logs.
- Vapour integration is staging-only in builds, backend, rules and deployment.
  Native standalone/local scope commands remain independent of that setting.
- Browser telemetry has no code dependency on this workspace.

## Inputs required before integration

- Vapour native runtime identity/version, frame marker, memory.grow handler,
  import trampolines, JIT symbol export, allocator hooks, local game PID registry.
- Existing staging/production origins, build/deploy configuration, Firebase
  projects, backend config/rules and game-owner authorization model.
- Web player lifecycle, memory export, tab/game/session identity and headers
  controlling isolation and JS self-profiling.

## Port-6767 agent update

Confirmed scope: **both native and browser telemetry**, synchronized with the
Vapour CLI through one local endpoint. The agent remains runtime-independent;
Vapour-specific game resolution and environment checks belong in the Go CLI.

The agent is local and toggle-owned. CLI and UI consume the same source-tagged
session status and metrics. Native and browser capture keep separate toggles,
buffers and TTLs. Off, TTL or target exit cleans up the corresponding capture;
the final active capture ending closes the endpoint and exits the agent.
Starting a browser-only session must not load or attach native BPF probes.

Browser telemetry stays an independent TypeScript package. Its optional local
agent export uses validated, bounded records from the pinned `.vpt` contract,
not Rust code or a shared analyzer. Browser-only capture, dump and direct
analysis must still work without installing the native profiler. Local forwarding
starts only on an explicit toggle; no off-state discovery or reconnection poll.

Keep token/origin checks despite the predictable port; fail clearly on a port
conflict without terminating the port's current owner. Bound ingress, clients,
queues, buffers and trace storage. Never create an always-on service or route
telemetry through Vapour servers. The implementation follows the lifecycle,
trace, API and CLI gates above, with frontend/backend staging guardrails first.

Do not guess these. The initial generic BPF proof does not need them.
