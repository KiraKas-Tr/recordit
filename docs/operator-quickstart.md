# Operator Quickstart (`bd-1nzn`)

Date: 2026-03-02  
Status: canonical happy-path quickstart for the human-first `recordit` CLI

## Goal

Get a new operator from zero to a successful session in four commands, without needing to learn legacy `transcribe-live` flags first.

## Prerequisites

- macOS host with Screen Recording and Microphone permissions available
- a local whisper.cpp model at `artifacts/bench/models/whispercpp/ggml-tiny.en.bin`

Bootstrap the default local model if needed:

```bash
make setup-whispercpp-model
```

## Happy Path

### 1. Check the machine and planned artifact root

```bash
cargo run --bin recordit -- preflight --mode live --json
```

What success looks like:

- `overall_status: PASS`
- session-scoped artifact root under `artifacts/sessions/<date>/<timestamp>-live/`
- top-level manifest mode labels:
  - `runtime_mode=live-stream`
  - `runtime_mode_taxonomy=live-stream`
  - `runtime_mode_selector=--live-stream`

### 2. Run the primary live operator path

```bash
cargo run --bin recordit -- run --mode live --model artifacts/bench/models/whispercpp/ggml-tiny.en.bin --json
```

What to look for:

- a concise `Startup banner`
- `run_status: ok|degraded`
- `remediation_hints: ...`
- trailing JSON output with the session artifact paths

Default live artifact layout:

- `session.input.wav`
- `session.wav`
- `session.jsonl`
- `session.manifest.json`

All of those land under the session root printed in the trailing JSON envelope.

### 3. Replay the session if you want machine-readable review

```bash
cargo run --bin recordit -- replay --jsonl <session-root>/session.jsonl --format json
```

Use replay when you want:

- deterministic event inspection
- transcript review without rerunning capture
- a machine-readable payload for automation

### 4. Use the offline fallback when you want deterministic local validation

```bash
cargo run --bin recordit -- run --mode offline --input-wav artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav --model artifacts/bench/models/whispercpp/ggml-tiny.en.bin --json
```

Use offline mode when:

- you want a reproducible local sanity check
- you do not need live capture
- you want to validate artifact output without permission/capture variables

## How To Read Results

- `run_status=ok`: session completed without trust notices
- `run_status=degraded`: session completed, but trust/degradation signals need operator review
- `run_status=failed`: command exited non-zero; read `remediation_hint=...` first

For degraded sessions:

- prefer `reconciled_final` over raw `final` output when reconciliation was applied
- inspect `trust`, `degradation_events`, and `session_summary` in `session.manifest.json`

## Legacy Note

`transcribe-live` remains supported for legacy scripts, gates, and expert workflows, but the normal operator path is now `recordit`.
