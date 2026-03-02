# Golden Artifact Matrix

Date: 2026-03-02
Bead: `bd-1qfx`
Status: frozen Phase A baseline for artifact-driven regression work

## Purpose

This document identifies the baseline manifest/JSONL examples that future Phase A and Phase B work should diff against.

It is the artifact companion to:

- `docs/runtime-contract-inventory.md`
- `docs/compatibility-boundary-policy.md`

The goal is not byte-for-byte replay of every path or benchmark number. The goal is to freeze the current public meaning of runtime artifacts so future work can detect contract drift mechanically.

## Baseline Directory

Canonical baseline directory:

- `artifacts/validation/bd-1qfx/`

Machine-readable indexes:

- `artifacts/validation/bd-1qfx/matrix.json`
- `artifacts/validation/bd-1qfx/matrix.csv`

Frozen scenario artifacts:

- `artifacts/validation/bd-1qfx/representative-offline.runtime.manifest.json`
- `artifacts/validation/bd-1qfx/representative-offline.runtime.jsonl`
- `artifacts/validation/bd-1qfx/representative-chunked.runtime.manifest.json`
- `artifacts/validation/bd-1qfx/representative-chunked.runtime.jsonl`
- `artifacts/validation/bd-1qfx/live-stream-cold.runtime.manifest.json`
- `artifacts/validation/bd-1qfx/live-stream-cold.runtime.jsonl`
- `artifacts/validation/bd-1qfx/live-stream-warm.runtime.manifest.json`
- `artifacts/validation/bd-1qfx/live-stream-warm.runtime.jsonl`
- `artifacts/validation/bd-1qfx/live-stream-backlog.runtime.manifest.json`
- `artifacts/validation/bd-1qfx/live-stream-backlog.runtime.jsonl`
- `artifacts/validation/bd-1qfx/live-stream-backlog.summary.csv`

## How This Baseline Was Assembled

- `representative-offline` was freshly generated from the current `target/debug/transcribe-live` binary against the deterministic stereo fixture.
- `representative-chunked` was freshly generated from the current binary using `RECORDIT_FAKE_CAPTURE_FIXTURE`.
- `live-stream-cold`, `live-stream-warm`, and `live-stream-backlog` were copied from the latest known passing artifacts:
  - `artifacts/bench/gate_v1_acceptance/20260301T130355Z/`
  - `artifacts/bench/gate_backlog_pressure/20260301T130103Z/`

This mix is intentional:

- the fresh runs freeze current representative behavior under the current binary
- the copied gate artifacts freeze the known-good live-stream acceptance surfaces that already passed the latest v1 gates

## Scenario Matrix

| Scenario | Source type | Runtime tuple | Key transcript counts | Trust/degradation expectation | Reconciliation expectation |
|---|---|---|---|---|---|
| `representative-offline` | fresh local run | `representative-offline` / `representative-offline` / `<default>` / `implemented` | `partial=2`, `final=2`, `reconciled_final=0` | nominal: `trust_notice_count=0`, `degradation_event_count=0` | `required=false`, `applied=false` |
| `representative-chunked` | fresh fake-capture run | `live-chunked` / `representative-chunked` / `--live-chunked` / `implemented` | `partial=22`, `final=2`, `reconciled_final=2` | degraded: trust codes `chunk_queue_backpressure`, `chunk_queue_backpressure_severe`, `reconciliation_applied`; degradation codes `live_chunk_queue_drop_oldest`, `live_chunk_queue_backpressure_severe`, `reconciliation_applied_after_backpressure` | `required=true`, `applied=true` |
| `live-stream-cold` | copied passing gate artifact | `live-stream` / `live-stream` / `--live-stream` / `implemented` | `partial=16`, `final=8`, `reconciled_final=0` | nominal: `trust_notice_count=0`, `degradation_event_count=0` | `required=false`, `applied=false` |
| `live-stream-warm` | copied passing gate artifact | `live-stream` / `live-stream` / `--live-stream` / `implemented` | `partial=16`, `final=8`, `reconciled_final=0` | nominal: `trust_notice_count=0`, `degradation_event_count=0` | `required=false`, `applied=false` |
| `live-stream-backlog` | copied passing backlog-pressure artifact | `live-stream` / `live-stream` / `--live-stream` / `implemented` | `partial=70`, `final=8`, `reconciled_final=0` | current passing profile is `buffered-no-drop`, so trust/degradation remain absent and `live-stream-backlog.summary.csv` is the canonical explanation | `required=false`, `applied=false` |

## Frozen Expectations Shared Across the Matrix

These expectations are intentionally redundant with `matrix.json` because future regression work should use them as the first semantic checks.

### Shared invariants

- `kind=transcribe-live-runtime` for all runtime scenarios
- `runtime_mode_status=implemented` for all runtime scenarios
- lifecycle order is `warmup|active|draining|shutdown`
- `out_wav_materialized=true` for all runtime scenarios
- JSONL contains `lifecycle_phase`, `vad_boundary`, transcript rows, `reconciliation_matrix`, `asr_worker_pool`, `chunk_queue`, and `cleanup_queue` for the current representative/live baselines

### Offline baseline

- `runtime_mode=representative-offline`
- no trust/degradation events
- no reconciliation
- current transcript surface contains stable `partial` and `final` rows without `reconciled_final`

### Representative-chunked baseline

- compatibility label remains `runtime_mode=live-chunked`
- taxonomy remains `runtime_mode_taxonomy=representative-chunked`
- trust/degradation/reconciliation example currently lives here
- this is the best frozen example for:
  - queue-drop behavior
  - `reconciled_final`
  - trust escalation codes
  - degradation codes tied to backlog pressure

### Live-stream cold and warm baselines

- both preserve the live-stream runtime tuple
- both prove first stable transcript emission during active runtime in the passing v1 gate set
- both are nominal examples with zero trust/degradation notices

### Live-stream backlog baseline

- this is the canonical artifact for the current accepted backlog-pressure surface
- the matching `live-stream-backlog.summary.csv` must be read together with the manifest/JSONL
- current accepted profile is `buffered-no-drop`, so a passing backlog scenario does not require degradation/trust rows

## What Future Diff Harnesses Should Compare

Good semantic comparison targets:

- runtime-mode tuple
- lifecycle phase sequence
- presence/absence of transcript/control event families
- transcript family counts
- trust/degradation code sets
- reconciliation required/applied booleans
- `out_wav_materialized`
- `terminal_summary.live_mode`
- `first_emit_timing_ms` shape and presence
- `event_counts`, `chunk_queue`, `cleanup_queue`, and `session_summary` key presence

Do not treat these as strict byte-for-byte baselines:

- absolute artifact paths such as `out_wav`, `out_jsonl`, `out_manifest`, `jsonl_path`
- timestamps such as `generated_at_utc` or lifecycle `entered_at_utc`
- benchmark output paths or raw benchmark numbers
- host-specific staging/helper paths

Those fields are still contractual in shape and meaning, but future diff tooling should normalize them before comparison.

## Recommended Use By Downstream Beads

`bd-1n5v`:

- use `matrix.json` as the baseline index
- compare semantic fields first, then deeper payloads

`bd-3ruu`:

- use the manifest files in this directory to assert stable top-level and nested key presence

`bd-10uo`:

- use the JSONL files in this directory to assert stable event families, required keys, and sequencing invariants

## Notes

- The current repo also contains older validation artifacts under `artifacts/validation/` that predate some of the now-frozen live-stream/runtime-mode fields.
- For new regression work, prefer the `bd-1qfx` baseline directory over older ad hoc artifacts.

