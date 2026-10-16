<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# `vp-report` JSON format — version 1

`vp-report` is a single JSON document derived from a `.vpt` trace.

- Schema: [`schema/vp-report-v1.schema.json`](schema/vp-report-v1.schema.json)
- Format tag: `"format": "vp-report"`
- Version: `1`

## Purpose

`vp-report` packages full retained trace data and computed metrics into one file.
Use it for dashboard export, CLI analysis, `vapour test`, and CI consumers that need
a single JSON payload.

## Main fields

- `metadata`: the original `.vpt` metadata record.
- `capture`: duration, per-record-type counts, and dropped counts from `end.dropped`.
- `frames`: frame count, FPS summary, frame-time percentiles/stats, histogram, spike list.
- `metrics.series`: every key seen in any `metrics.values` appears as a time series.
- `metrics.stats`: generic stats per metric key (`min`, `max`, `mean`, `start`, `end`, `bytesPerSecond`).
- `cpu`: average and peak `cpuOneCorePercent`.
- `memory`: `memory` grow events and per-kind metric stats.
- `functions`: stack-derived self-sample ranking.
- `stacks`: folded stacks with sample counts.
- `assets`: full `asset_report` output (not truncated).
- `spikes`: raw `spike` records.
- `probes`: raw `probe` records.
- `trace`: raw `.vpt` records in original order (omitted when `--no-trace` / `trace=0`).

## Methods

- **Percentiles:** nearest-rank.
- **FPS lows:** inverse mean frame duration over the slowest `ceil(1%)` and `ceil(0.1%)` frames.
- **Histogram buckets (ms):** `8.3`, `16.7`, `33.3`, `50`, `100`, and `∞`
  (`∞` is encoded as `"upTo": null`).

## Approximation notes

Derived values are approximate:

- slope-style rates (`bytesPerSecond`) are regression estimates from sampled points;
- correlated CPU/memory asset costs are inferred from 1 Hz metrics;
- retained samples are not CPU utilization percentages.

Unsupported metrics stay `null` or absent; they are never synthesized as `0`.
