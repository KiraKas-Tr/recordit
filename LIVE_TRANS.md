# LIVE_TRANS: True Concurrent Live Capture + Transcription

## Summary

Implement a real `--live-stream` runtime that performs capture and transcription concurrently:

1. Start command
2. Capture audio continuously
3. Build chunks from captured stream while capture is still running
4. Transcribe and print to terminal during capture
5. Continue capture + chunking + transcription until stop/drain

This replaces the current serialized behavior where capture completes first and transcript output appears only after timeout.

## Current Gap (Must Be Fixed)

Current `--live-stream` flow is not truly live:

- `run_live_stream_pipeline()` aliases to chunked/post-capture path.
- capture is fully completed before ASR scheduling/terminal emission.
- terminal stream is replay of precomputed events.

As a result, users only see transcript output after `LIVE_STREAM_SECS` ends.

## Product Behavior Target

For `transcribe-live --live-stream`:

- capture starts immediately.
- transcript output appears before capture duration ends.
- default terminal mode is stable final lines only.
- default channel mode is separate (`mic`, `system`).
- `--input-wav` is progressively materialized during session.
- `--out-wav` remains canonical success artifact.
- JSONL/manifest remain deterministic and durable.
- `--live-chunked` remains representative-chunked legacy path.

## Compatibility Contract

### Keep

- Existing flag names and compatibility checks.
- Existing representative-offline and representative-chunked behavior.
- Existing whispercpp backend default.
- Existing additive artifact policy and trust/degradation semantics.

### Change

- `--live-stream` becomes true concurrent runtime (not alias).
- `--input-wav` in live-stream becomes progressive live scratch artifact.

## Architecture Plan

## 1) Capture: Add Streaming API

### Files

- `src/live_capture.rs`
- `src/capture_api.rs`

### Additions

Add to `capture_api.rs`:

- `CaptureMessage`
  - `Chunk(CaptureChunk)`
  - `Event(CaptureEvent)`
  - `Finished(CaptureSummary)`

- `CaptureSink` trait
  - `on_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String>`
  - `on_event(&mut self, event: CaptureEvent) -> Result<(), String>`

- `StreamingCaptureResult`
  - `summary: CaptureSummary`
  - `progressive_output_path: PathBuf`

Add to `live_capture.rs`:

- `run_streaming_capture_session(config: &LiveCaptureConfig, sink: &mut dyn CaptureSink) -> Result<StreamingCaptureResult>`

### Behavior

- Reuse current callback + transport model.
- Emit normalized mono chunks with stable timing metadata while capturing.
- Continue progressive WAV materialization to `config.output` during runtime.
- Emit degradation/interruption events immediately.
- Keep callback path non-blocking.

### Preserve existing recorder behavior

- Keep `run_capture_session()` public behavior unchanged.
- Implement it as wrapper over streaming session with collector sink.

## 2) Fake Capture Harness: Timed Replay

Current fake mode copies fixture instantly. Replace for streaming runtime with timed chunk replay:

- Read stereo fixture.
- Emit mic/system chunks in timestamp order.
- Support real-time and accelerated replay.

Env knobs:

- `RECORDIT_FAKE_CAPTURE_FIXTURE`
- `RECORDIT_FAKE_CAPTURE_RESTART_COUNT`
- `RECORDIT_FAKE_CAPTURE_REALTIME=1|0`

Default:

- accelerated for tests.
- real-time optional for smoke/manual validation.

## 3) ASR: Long-Lived Service (Not Batch)

### File

- `src/live_asr_pool.rs`

### Problem

`run_live_asr_pool()` is batch-oriented (`Vec<LiveAsrJob>` in, wait for all out). Not suitable for continuous live intake.

### Add service API

- `LiveAsrService`
  - `prewarm_once()`
  - `try_submit_partial(job)`
  - `try_submit_reconcile(job)`
  - `try_submit_final(job)`
  - `try_recv_result()`
  - `close()`
  - `join()`

### Queue policy

- `final_queue` (bounded, highest priority, never dropped)
- `background_queue` for partial/reconcile
  - if full: drop oldest partial first
  - then oldest reconcile
  - final jobs must not be dropped

If final queue temporarily full:

- coordinator buffers pending finals and retries on next loop tick.
- never block capture.

Temp audio policy remains consistent with `--keep-temp-audio`.

## 4) New Live Coordinator Runtime Module

### New module

- `src/live_stream_runtime.rs`

### Responsibilities

- ingest capture chunks.
- maintain per-channel rolling buffers.
- incremental VAD tracking.
- schedule ASR windows/finals.
- submit jobs to ASR service.
- collect results and emit deterministic events.
- render terminal output during runtime.
- stream JSONL incrementally.
- finalize manifest and summary on drain.

## 5) Scheduling & VAD Model

Defaults:

- chunk window: `2000ms`
- chunk stride: `500ms`
- channel mode: `separate`
- terminal mode: final-only

### VAD

Use incremental per-channel VAD over incoming samples (not post-run WAV pass):

- open segment when `min_speech_ms` satisfied.
- close segment when `min_silence_ms` satisfied.
- on shutdown: flush open speech segments (`shutdown_flush`).

### Job generation

- Partial jobs on stride while segment open.
- Final job when segment closes.
- Reconcile jobs only when needed (degradation/failure conditions).

## 6) Deterministic Ordering Rules

Concurrency must not break replay determinism.

Add additive ordering fields for emitted events:

- `emit_seq`
- `segment_ord`
- `window_ord`
- `job_class`

Rules:

- final lane never blocked by late partials.
- lane-local ordering deterministic.
- merged output deterministic by stable sort keys.

## 7) Terminal Output Rules

For live-stream default:

- print stable lines (`final`, `reconciled_final`) during capture.
- do not print partial overwrite lines by default.
- preserve existing stable line format.

End summary should not duplicate full transcript replay when already printed live.

## 8) Artifact Semantics

### `--input-wav`

- progressive live scratch artifact (grows during run).

### `--out-wav`

- canonical final artifact, always materialized on success.

### JSONL

Write incrementally during runtime:

- lifecycle transitions
- VAD boundaries
- transcript events
- queue telemetry
- degradation/trust notices
- capture continuity events

### Manifest

Write final manifest at close using accumulated runtime state.

Additive live fields (if not already present):

- `runtime_mode*` labels
- first emit timing metrics
- queue backlog/defer metrics
- terminal render mode

## 9) Runtime Refactor in `transcribe_live.rs`

Required split:

- representative-offline path
- representative-chunked path
- true live-stream path

### Key required changes

- Replace `run_live_stream_pipeline()` alias behavior with real coordinator orchestration.
- Do not call post-capture `prepare_runtime_input_wav()` in live-stream path.
- Keep existing non-live paths unchanged.

## 10) Testing Plan

## Unit tests

- streaming capture emits chunks/events correctly.
- fake streaming harness chunk timing deterministic.
- incremental VAD boundaries from streamed samples.
- ASR queue policy: drop partial first, never drop final.
- deterministic event ordering despite out-of-order completions.
- terminal final-only rendering mode.

## Integration tests

- live-stream deterministic fixture run emits first final before timeout.
- JSONL grows during active capture.
- input WAV exists and grows during run.
- backpressure scenario preserves capture continuity and emits trust/degradation notices.
- long uninterrupted speech flush behavior on shutdown.

## Gate updates

Update v1 acceptance gates to validate real live behavior:

- first transcript emission during capture.
- non-blocking capture under ASR pressure.
- artifact truth and trust/degradation surfacing.

## 11) Manual Verification Commands (Post-Implementation)

### Real host live run

```bash
make transcribe-live-stream \
  TRANSCRIBE_LIVE_STREAM_SECS=30 \
  ASR_MODEL=artifacts/bench/models/whispercpp/ggml-tiny.en.bin
```

Expected:

- terminal lines appear before 30s ends.
- `artifacts/transcribe-live-stream.input.wav` grows during run.
- JSONL grows during run.

### Deterministic fake-live run

```bash
RECORDIT_FAKE_CAPTURE_FIXTURE=artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav \
RECORDIT_FAKE_CAPTURE_REALTIME=1 \
cargo run --bin transcribe-live -- \
  --duration-sec 8 \
  --live-stream \
  --input-wav artifacts/live.input.wav \
  --out-wav artifacts/live.session.wav \
  --out-jsonl artifacts/live.session.jsonl \
  --out-manifest artifacts/live.session.manifest.json \
  --asr-model artifacts/bench/models/whispercpp/ggml-tiny.en.bin
```

## 12) Rollout Sequence

1. Add streaming fake-capture timed replay.
2. Add streaming capture API and sink plumbing.
3. Add long-lived ASR service.
4. Implement live coordinator module.
5. Wire `run_live_stream_pipeline()` to coordinator.
6. Stream JSONL + terminal output during run.
7. Update tests and gates.
8. Update docs (`README` + contracts) to reflect true behavior.

## Assumptions / Defaults Locked

- Live backend in scope: whispercpp.
- Moonshine remains out-of-scope for this capability.
- Terminal default: final-only.
- Channel default: separate.
- Backpressure: drop partial/reconcile first, never block capture.
- `--live-chunked` stays for representative compatibility.

