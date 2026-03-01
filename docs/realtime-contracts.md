# Real-Time Callback Contract and Recovery Matrix

## Callback Contract (Recorder/Probe)

The ScreenCaptureKit callback path must remain deterministic and non-blocking:

1. No disk I/O in callback handlers.
2. No blocking waits (`recv`, locks, sleeps) in callback handlers.
3. No unbounded queue growth; all callback handoff uses fixed-capacity preallocated slots.
4. Any sample-format contract violation is counted and mapped to an explicit recovery action.

Enforcement points:
- `src/rt_transport.rs` tracks pressure and drops (`slot_miss_drops`, `queue_full_drops`, `ready_depth_high_water`, `in_flight`).
- `src/bin/sequoia_capture.rs` tracks callback contract violations:
  - `missing_audio_buffer_list`
  - `missing_first_audio_buffer`
  - `missing_format_description`
  - `missing_sample_rate`
  - `non_float_pcm`
  - `chunk_too_large`
- `src/bin/sequoia_capture.rs` telemetry also records `sample_rate_policy` with mismatch mode, input rates, and per-channel resampling counters.
- `src/bin/transcribe_live.rs` near-live runtime path reuses recorder capture primitives by calling the shared `recordit::live_capture` session runtime for `--live-chunked`, avoiding duplicate callback-thread logic in transcribe.
- `src/bin/transcribe_live.rs` reads near-live capture telemetry (`restart_count`) and converts interruption recovery or missing continuity telemetry into explicit degradation/trust notices so continuity impact is never silent.

## Cleanup Queue Contract (Transcribe)

Finalized-segment cleanup is an optional post-processing lane and must never block ASR event emission:

1. Only `final` transcript events are eligible for cleanup.
2. Cleanup submission uses bounded non-blocking enqueue (`try_send` on a fixed-capacity queue).
3. Queue-full submissions are dropped and counted instead of waiting.
4. Per-request execution is constrained by `--llm-timeout-ms` and `--llm-retries`.
5. Runtime drain is budgeted; unfinished cleanup work is reported as pending rather than delaying transcript completion indefinitely.

Operational knobs:
- `--llm-cleanup`
- `--llm-endpoint`
- `--llm-model`
- `--llm-timeout-ms`
- `--llm-max-queue`
- `--llm-retries`

Telemetry/artifact surface:
- terminal summary prints cleanup queue totals (`submitted`, `enqueued`, `dropped_queue_full`, `processed`, `succeeded`, `timed_out`, `failed`, `retry_attempts`, `pending`, `drain_completed`)
- runtime JSONL emits a terminal `cleanup_queue` control event
- runtime manifest persists the same cleanup queue summary under `cleanup_queue`

## ASR Worker Pool Contract (Transcribe)

Channel transcription work runs through a dedicated worker pool so ASR concurrency, prewarm behavior, and temp-audio lifecycle are explicit and testable.

Worker + lifecycle semantics:
1. channel ASR jobs are submitted through `run_live_asr_pool(...)` using bounded non-blocking enqueue.
2. each run performs backend prewarm probe before channel jobs execute (`whisper-cli -h` / `whisperkit-cli --help`), and prewarm failures fail the run explicitly.
3. channel-slice temp WAVs are marked as temp artifacts and follow `RetainOnFailure` cleanup policy:
   - success path deletes temp slices.
   - failure path retains temp slices for debug inspection.
4. representative benchmark loops that run multiple passes refresh channel temp slices per pass so cleanup policy stays deterministic across runs.

Telemetry/artifact surface:
- terminal summary prints `asr_worker_pool` totals (`prewarm_ok`, `submitted`, `enqueued`, `dropped_queue_full`, `processed`, `succeeded`, `failed`, `retry_attempts`, `temp_audio_deleted`, `temp_audio_retained`)
- runtime JSONL emits `asr_worker_pool` control events
- runtime manifest persists `asr_worker_pool` under top-level reliability telemetry

## Near-Live Chunk Queue Contract (Transcribe)

Near-live chunk scheduling must keep producer behavior non-blocking even when ASR chunk work lags:

1. Chunk work submission uses a bounded queue (`--chunk-queue-cap`).
2. ASR work items are explicitly classed as `final`, `partial`, or `reconcile`.
3. `partial` work items are scheduled at a fixed rolling cadence over bounded context (default `--chunk-window-ms=2000`, `--chunk-stride-ms=500`).
4. Strict queue priority is enforced under pressure: `final` > `reconcile` > `partial`.
5. Queue saturation evicts the **oldest lowest-priority** queued item (never blocking the producer).
6. Sustained drop pressure escalates to an explicit severe-path degradation signal.
7. Drop counts are surfaced as degradation/trust signals rather than silent data loss.
8. Segment IDs are canonicalized from time-ordered boundaries + channel order so identical inputs produce identical `partial`/`final` identifiers.

Telemetry/artifact surface:
- terminal summary prints near-live `chunk_queue` totals (`submitted`, `enqueued`, `dropped_oldest`, `processed`, `pending`, `high_water`, `drain_completed`) plus lag stats (`lag_sample_count`, `lag_p50_ms`, `lag_p95_ms`, `lag_max_ms`)
- runtime JSONL emits `chunk_queue` control events
- runtime JSONL emits trust notices for queue pressure, including severe-path escalation (`chunk_queue_backpressure_severe`) when sustained drops are detected
- runtime manifest persists `chunk_queue` under top-level reliability telemetry
- canonical `out_wav` is progressively materialized during live capture (consumer-side snapshots) and finalized at session end
- runtime manifest persists out-wav truth fields (`out_wav`, `out_wav_materialized`, `out_wav_bytes`) for gate-safe artifact validation

## Incremental VAD Contract (Transcribe)

VAD segmentation is tracked incrementally per channel using threshold + min speech/silence durations so boundaries stay stable under streaming-style progression.

Boundary semantics:
1. speech-open requires `min_speech_ms` consecutive above-threshold frames per channel.
2. speech-close requires `min_silence_ms` consecutive below-threshold frames per channel.
3. runtime boundary timeline remains deterministic by unioning per-channel intervals into ordered merged boundaries.
4. each merged boundary closure emits exactly one `final` ASR work item per channel (no duplicate final jobs for the same boundary).
5. if capture ends while speech is still open, the trailing boundary is force-closed during tracker finish with `source=shutdown_flush` so spoken tails are not dropped.

## Live Lifecycle Contract (Transcribe)

Live runtime execution must expose explicit lifecycle transitions so operators can distinguish startup from active emission and deterministic finalization.

Lifecycle phases:
1. `warmup`: model/capture/channel preparation is in progress and transcript emission is not yet ready.
2. `active`: warmup is complete; transcript chunks/finals may emit.
3. `draining`: active emission has ended and cleanup/reconciliation/final assembly are being finalized.
4. `shutdown`: runtime execution is complete and session artifacts are being persisted.

Telemetry/artifact surface:
- terminal summary prints lifecycle status (`current_phase`, `ready_for_transcripts`, transition count) plus ordered transition rows
- runtime JSONL emits ordered `lifecycle_phase` control events (`phase`, `transition_index`, `entered_at_utc`, `ready_for_transcripts`, `detail`)
- runtime manifest persists lifecycle state under top-level `lifecycle` with current phase and ordered transition history

## Reconciliation Trigger Matrix (Transcribe)

Reconciliation is now driven by an explicit trigger matrix so runtime behavior is deterministic and auditable.

Matrix rules:
1. run reconciliation when chunk queue pressure dropped live work (`chunk_queue_drop_oldest`).
2. run reconciliation when continuity risk is present (`continuity_recovered_with_gaps` or `continuity_unverified`).
3. run reconciliation when any boundary was force-closed at shutdown (`shutdown_flush_boundary`).
4. when triggered, reconciliation must target affected boundary regions:
   - queue-drop trigger: prefer boundaries with missing live `final` segment IDs
   - shutdown trigger: include shutdown-flush boundaries
   - continuity triggers: include all boundaries unless finer-grained impact data exists
5. skip reconciliation when no trigger fires; live `final` output is considered sufficient.

Telemetry/artifact surface:
- terminal summary prints `reconciliation_matrix` (`required`, `applied`, `trigger_count`, `trigger_codes`)
- runtime JSONL emits a `reconciliation_matrix` control event
- runtime manifest persists top-level `reconciliation` state with trigger codes
- when reconciliation runs, runtime still emits `mode_degradation` + `trust_notice` for `reconciliation_applied`

## Trust Notice Guidance Mapping (Transcribe)

Trust notices must stay actionable for operators and directly traceable to machine artifacts.

| Trust notice code | Typical trigger path | Operator guidance focus | Artifact trace |
|---|---|---|---|
| `continuity_recovered_with_gaps` | capture restart recovered from interruption | review continuity timeline before treating session as gap-free | `mode_degradation` + `trust_notice` + runtime `lifecycle` |
| `continuity_unverified` | continuity telemetry missing/unreadable | restore capture telemetry readability and rerun for verification | `mode_degradation` + `trust_notice` + capture telemetry metadata |
| `chunk_queue_backpressure` | queue saturation dropped oldest chunk work | increase `--chunk-queue-cap` or reduce near-live load | `chunk_queue` + `mode_degradation` + `trust_notice` |
| `chunk_queue_backpressure_severe` | sustained queue pressure and drops | treat incremental transcript as degraded and rely on reconciled/offline review | `chunk_queue` lag/drop counters + severe degradation/trust events |
| `reconciliation_applied` | trigger matrix required reconciliation | treat `reconciled_final` as canonical and inspect `reconciliation_matrix.trigger_codes` | `reconciliation_matrix` + `reconciled_final` + `trust_notice` |

## Runtime Mode Taxonomy and Compatibility Matrix

`transcribe-live` mode semantics are intentionally split into taxonomy mode vs artifact/runtime label so downstream tooling can evolve without schema churn.

| Taxonomy mode | Current selector | Runtime artifact label (`runtime_mode`) | Status | Replay (`--replay-jsonl`) | Preflight (`--preflight`) | Chunk tuning (`--chunk-*`) |
|---|---|---|---|---|---|---|
| `representative-offline` | `<default>` | `representative-offline` | implemented | compatible | compatible | forbidden |
| `representative-chunked` | `--live-chunked` | `live-chunked` (kept for compatibility) | implemented | incompatible | incompatible | compatible |
| `live-stream` | `--live-stream` | `live-stream` | implemented | incompatible | incompatible | compatible |

Additive manifest/preflight fields that carry this contract:
- `runtime_mode_taxonomy`
- `runtime_mode_selector`
- `runtime_mode_status`

Debug/operator path expectation:
- debug CLI (`cargo run --bin transcribe-live ...`) and packaged operator paths (`make run-transcribe-app`) must report the same mode semantics and compatibility behavior.
- `--live-stream` and `--live-chunked` remain mutually exclusive selectors so runtime intent is explicit and non-ambiguous.

## JSONL Event Contract

Canonical event semantics live in `docs/live-jsonl-event-contract.md`.

Required invariants:
1. Runtime JSONL is append-only and emitted incrementally as lifecycle and transcript stages complete (not only at end-of-run).
2. Durability checkpoints call `sync_data()` every 24 lines and at lifecycle/stage boundaries to improve crash survivability.
3. Transcript event ordering remains deterministic for replay/readable reconstruction (`start_ms`, `end_ms`, event rank, `channel`, `segment_id`, `source_final_segment_id`, `text`).
4. Derived transcript events (`llm_final`, `reconciled_final`) carry `source_final_segment_id` when lineage exists.
5. Replay consumes transcript events plus `trust_notice`, while other control events remain machine-facing context rather than replay input.

Deterministic gate evidence:
- `scripts/gate_backlog_pressure.sh` induces near-live queue pressure and evaluates degradation/trust thresholds
- `scripts/gate_transcript_completeness.sh` measures replay-readable completeness gain before/after reconciliation under induced backlog

## Near-Live Terminal UX Contract

Near-live terminal behavior is governed by `docs/near-live-terminal-contract.md`.

Key guarantees from that contract:

1. Concise default output is terminal-aware:
   - interactive `TTY` emits low-noise partial overwrite updates plus stable transcript lines
   - non-`TTY` falls back to deterministic stable-only transcript lines
   - end-of-session summary suppresses duplicate replay of stable lines already emitted during active runtime
2. Verbose diagnostics is opt-in and must not change runtime semantics.
3. End-of-session summary fields are deterministic and ordered for consistent human and automated review.
4. Trust/degradation notices are user-actionable and mapped to machine artifacts.
5. Runtime manifest exposes aligned `terminal_summary` metadata (`live_mode`, `stable_line_count`, `stable_lines_replayed`, `stable_lines`) for machine audit of summary behavior.

## Error to Recovery Matrix

| Error Class | Detection | Recovery Action |
|---|---|---|
| Callback slot unavailable | `slot_miss_drops` increment | `DropSampleContinue` |
| Ready queue full | `queue_full_drops` increment | `DropSampleContinue` |
| Missing audio buffer list | callback contract counter | `DropSampleContinue` |
| Missing first audio buffer | callback contract counter | `DropSampleContinue` |
| Missing format description | callback contract counter | `DropSampleContinue` |
| Missing sample rate | callback contract counter | `DropSampleContinue` |
| Non-float PCM | callback contract counter | `FailFastReconfigure` |
| Chunk exceeds slot capacity | callback contract counter | `DropSampleContinue` |
| Stream interruption with restarts remaining | idle-gap + restart budget | `RestartStream` |
| Stream interruption with restart budget exhausted | idle-gap + restart budget | `FailFastReconfigure` |
| Sample-rate mismatch in `strict` mode | policy check | `FailFastReconfigure` |
| Sample-rate mismatch in `adapt-stream-rate` mode | policy check + worker resampling to canonical output rate | `AdaptOutputRate` |

## Validation

Current contract/recovery logic is validated with:
- transport unit tests (`cargo test --lib`)
- recorder policy tests (`DYLD_LIBRARY_PATH=/usr/lib/swift cargo test --bin sequoia_capture -- --nocapture`)
- transport stress harness (`cargo run --quiet --bin transport_stress -- --iterations 50000 --capacity 128 --payload-bytes 2048 --consumer-delay-micros 20`)
