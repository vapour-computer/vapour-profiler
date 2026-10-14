<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# `.vpt` trace format — version 1

UTF-8 JSON Lines: one object per line, no compression, maximum 50 MiB per file
and 1 MiB per line. Every record validates against
[`schema/vpt-v1.schema.json`](schema/vpt-v1.schema.json). Consumers should pin
the schema's immutable Git commit and SHA-256, not the moving `main` URL.
This is a data contract; producers need no profiler code dependency.

The first record is exactly one `metadata` record with `format: "vpt"` and
`version: 1`. Subsequent records are `metrics`, `frame`, `memory`, `stack`,
`spike` or opt-in `probe`. A completed dump ends with exactly one `end` record.
A partial, interrupted capture may omit `end`; readers must label it partial.
Reject duplicate metadata, unsupported versions, records after end, oversized
input, and non-finite numbers. Times are milliseconds relative to capture start,
monotonic within each record kind; different kinds can be grouped. Wall-clock
`startedAt` is ISO 8601. Durations and counts must be nonnegative.

- **Metadata:** source (`browser`, `native`, or another producer name), profile,
  optional adapter/runtime/kernel/CPU identity and feature-detection capabilities.
- **Metrics:** 1Hz `values`. Unsupported fields are absent or null, never invented
  zeros. FPS is frames/second; lows are inverse mean duration of the slowest
  ceil(1%)/ceil(0.1%) of frames in the bucket. Memory is bytes. Browser main-thread
  occupancy is a lower-bound observer estimate, never CPU utilization. When
  capability `jsHeap` is `document-estimate`, `jsHeapBytes` actually represents
  the user-agent's document/worker memory estimate and must be labeled as such.
- **Frames:** duration and end timestamp; spike means greater than twice the
  preceding rolling median (up to 120 frames, at least 10 for warm-up). Hidden-tab
  gaps are not game frames. Browser rAF is presentation cadence, not GPU timing.
- **Memory:** observed size increases, from/to bytes. A browser's 1Hz observations
  may coalesce multiple grows; correlate within a one-second window, not as exact
  event timestamps. Replacement/decrease of a memory instance is not a grow.
- **Stacks:** folded symbol path (`caller;callee`) and positive sample count.
  A single symbol is a valid one-frame stack; no unavailable callers are fabricated.
- **Spikes:** bounded contextual windows; available stacks, thread CPU and memory
  events only. Optional native probe aggregates use the `probe` record.
- **End:** capture duration and dropped/overwritten record counts by kind. A
  bounded ring dump contains only retained data, and summaries must say so.

For `.folded` export, write `stack sample-count` per line. Replace literal newline
characters in symbol names; semicolons delimit frames.

## Implementation status

The browser producer uses this contract. The Rust analyzer and native trace
writer are not implemented yet; schema validation is not a claim that the
current `vapour-profiler` binary can analyze traces. That acceptance check is
required when the analyzer lands.
