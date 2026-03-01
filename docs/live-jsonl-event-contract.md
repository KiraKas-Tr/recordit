# Live JSONL Event Contract (bd-fey)

Date: 2026-03-01
Status: normative contract for replay-safe transcript artifacts

## Scope

Defines the runtime JSONL event schema emitted by `transcribe-live` for representative-offline, representative-chunked, and live-stream execution.

This contract covers:
- transcript events (`partial`, `final`, `llm_final`, `reconciled_final`)
- control events (`vad_boundary`, `mode_degradation`, `trust_notice`, `lifecycle_phase`, `reconciliation_matrix`, `asr_worker_pool`, `chunk_queue`, `cleanup_queue`)
- incremental append + durability checkpoint semantics
- deterministic transcript ordering and lineage rules required for replay safety

## Event Families

### Transcript events

Transcript rows share this base schema:
- required: `event_type`, `channel`, `segment_id`, `start_ms`, `end_ms`, `text`
- optional: `source_final_segment_id`
- required runtime context fields: `asr_backend`, `vad_boundary_count`

Allowed transcript `event_type` values:
- `partial`: non-terminal incremental transcript preview
- `final`: stable ASR segment output from the live/representative path
- `llm_final`: cleanup/refinement output with lineage to a `final` segment
- `reconciled_final`: post-session canonical correction event used when reconciliation is applied

ASR work item class mapping:
- `partial` work item class emits `partial` transcript events on rolling window cadence (default window/stride: `2000ms/500ms`)
- `final` work item class emits stable `final` output on merged VAD boundary closure (and may emit a paired `partial` preview for the same segment)
- `reconcile` work item class emits `reconciled_final` with lineage back to the source `final`
- reconciliation output should be boundary-targeted to affected regions (missing-live-final boundaries, shutdown-flush boundaries, or full-boundary sweep when continuity impact is global/unknown)
- queue pressure policy preserves higher-integrity classes first (`final` > `reconcile` > `partial`)
- live segment-id conventions are deterministic:
  - `partial`: `<role>-segment-<boundary_idx>-partial-<window_idx>-<start_ms>-<end_ms>`
  - `final`: `<role>-segment-<boundary_idx>-<start_ms>-<end_ms>`
  - IDs are generated after canonical boundary/channel ordering, so replay/human audit is stable even if upstream input vectors arrive in different orders.
- boundary-scoped `final` segment IDs are derived from deterministic boundary ordering (`start_ms`, `end_ms`, `source`, `id`) so ID stability does not depend on upstream boundary insertion order

Lineage rule:
- `llm_final` and `reconciled_final` SHOULD include `source_final_segment_id` when derived from a prior `final` segment.
- Missing lineage for derived events is treated as degraded provenance and should be considered a contract violation in new emitters.

### Control events

Control rows are append-only and must keep stable key names.

- `vad_boundary`:
  - required: `event_type`, `channel`, `boundary_id`, `start_ms`, `end_ms`, `source`, `vad_backend`, `vad_threshold`
  - `source` values:
    - `energy_threshold`: closed by silence threshold
    - `shutdown_flush`: forced close at session end to flush trailing open speech
- `mode_degradation`:
  - required: `event_type`, `channel`, `requested_mode`, `active_mode`, `code`, `detail`
- `trust_notice`:
  - required: `event_type`, `channel`, `code`, `severity`, `cause`, `impact`, `guidance`
  - queue-pressure codes include:
    - `chunk_queue_backpressure` (warn)
    - `chunk_queue_backpressure_severe` (error escalation when sustained drop pressure is detected)
  - continuity/reconciliation guidance codes include:
    - `continuity_recovered_with_gaps` (warn; restart recovery with potential gap boundaries)
    - `continuity_unverified` (warn; continuity confidence could not be fully established)
    - `reconciliation_applied` (warn; post-session reconciliation produced canonical `reconciled_final` output)
  - guidance text should remain operator-actionable and reference the canonical artifact path when relevant:
    - queue pressure: tune `--chunk-queue-cap` / load
    - continuity uncertainty: verify capture continuity telemetry and rerun
    - reconciliation: prioritize `reconciled_final` and inspect `reconciliation_matrix.trigger_codes`
- `lifecycle_phase`:
  - required: `event_type`, `channel`, `phase`, `transition_index`, `entered_at_utc`, `ready_for_transcripts`, `detail`
- `reconciliation_matrix`:
  - required: `event_type`, `channel`, `required`, `applied`, `trigger_count`, `trigger_codes`
  - `trigger_codes` can include:
    - `chunk_queue_drop_oldest`
    - `continuity_recovered_with_gaps`
    - `continuity_unverified`
    - `shutdown_flush_boundary`
- `asr_worker_pool`:
  - required: `event_type`, `channel`, worker telemetry (`prewarm_ok`, `submitted`, `enqueued`, `dropped_queue_full`, `processed`, `succeeded`, `failed`, `retry_attempts`, `temp_audio_deleted`, `temp_audio_retained`)
- `chunk_queue`:
  - required: `event_type`, `channel`, queue counters and lag fields (`lag_sample_count`, `lag_p50_ms`, `lag_p95_ms`, `lag_max_ms`)
- `cleanup_queue`:
  - required: `event_type`, `channel`, queue counters, retries, timeout, drain state

## Runtime Append and Durability

### Incremental append sequence

Runtime JSONL is emitted incrementally (append-only) during pipeline progression.

The writer emits rows in this stage-stable sequence:
1. `lifecycle_phase` transitions as phases are entered (`warmup`, `active`, `draining`, `shutdown`)
2. `vad_boundary` rows after VAD boundaries are known
3. initial transcript rows from live/representative emission (`partial` / `final`)
4. late transcript additions (`reconciled_final`, `llm_final`) when produced during draining
5. remaining control rows (`mode_degradation`, `trust_notice`, `reconciliation_matrix`, `asr_worker_pool`, `chunk_queue`, `cleanup_queue`)

### Durability checkpoints

- the writer calls `sync_data()` every `24` emitted lines
- the writer also checkpoints (`sync_data()`) at lifecycle/stage boundaries
- finalization performs one last durability sync
- crash recovery expectation: JSONL may be truncated at the latest durability checkpoint but must remain line-oriented and replay-parseable

### Transcript ordering key

When transcript events are merged/sorted (for replay and readable reconstruction), ordering must be:
1. `start_ms`
2. `end_ms`
3. `event_type` rank (`partial` < `final` < `reconciled_final` < `llm_final`)
4. `channel`
5. `segment_id`
6. `source_final_segment_id`
7. `text`

This ordering key must stay deterministic for identical input artifacts.

## Replay Semantics

Replay consumes JSONL as follows:
- consumed transcript event types: `partial`, `final`, `llm_final`, `reconciled_final`
- consumed control event type: `trust_notice` (printed as trust context)
- ignored control event types: `vad_boundary`, `mode_degradation`, `lifecycle_phase`, `reconciliation_matrix`, `asr_worker_pool`, `chunk_queue`, `cleanup_queue`

Replay safety invariants:
- missing `segment_id`, `start_ms`, or `end_ms` on transcript events is a hard replay error
- replay must preserve `source_final_segment_id` when present
- transcript reconstruction prefers `reconciled_final` over `final` for end-user readable output when reconciled events exist

## Mode Compatibility

`runtime_mode_taxonomy` and this JSONL contract are complementary:
- `representative-offline`: transcript/control event schema remains valid
- `representative-chunked`: adds pressure/reconciliation behaviors while preserving schema compatibility
- `live-stream` (implemented): remains additive to this contract (no breaking schema changes)

## Validation Expectations

Contract changes should include targeted regression coverage in `src/bin/transcribe_live.rs` for:
- lineage emission on `reconciled_final` and `llm_final`
- deterministic sort order with lineage tie-breaks
- replay parser acceptance/rejection behavior
- schema compatibility for near-live artifacts
