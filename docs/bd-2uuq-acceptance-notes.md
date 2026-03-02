# Operator-Path Acceptance Pass (bd-2uuq)

Date: 2026-03-02

## Scope

Validate the new operator happy path using canonical `recordit` commands and verify:
- first-run walkthrough viability
- startup/close-summary status surfaces
- artifact truth and replay usability
- rough edges that still need cleanup

## Inputs Used

- model: `artifacts/bench/models/whispercpp/ggml-tiny.en.bin`
- deterministic fixture: `artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav`
- output roots:
  - `/tmp/bd-2uuq-preflight`
  - `/tmp/bd-2uuq-offline`
  - `/tmp/bd-2uuq-live`

## Walkthrough Commands and Results

1. `cargo run --bin recordit -- --help`
- PASS
- command surface is concise (`run`, `doctor`, `preflight`, `replay`, `inspect-contract`)

2. `cargo run --bin recordit -- preflight --mode live --output-root /tmp/bd-2uuq-preflight --json`
- PASS (`overall_status=PASS`)
- output remains operator-readable and machine-consumable

3. `cargo run --bin recordit -- run --mode offline --input-wav <fixture> --output-root /tmp/bd-2uuq-offline --model <model> --json`
- PASS
- startup banner emitted deterministic field order
- close summary emitted deterministic order with `run_status=ok` and `remediation_hints=<none>`
- canonical artifact quartet materialized under session root

4. `RECORDIT_FAKE_CAPTURE_FIXTURE=<fixture> RECORDIT_FAKE_CAPTURE_REALTIME=0 cargo run --bin recordit -- run --mode live --output-root /tmp/bd-2uuq-live --model <model> --json`
- PASS
- `runtime_mode=live-stream` contract fields preserved
- deterministic close summary emitted with `run_status=ok` and no trust/degradation failures
- canonical artifacts present (`session.input.wav`, `session.wav`, `session.jsonl`, `session.manifest.json`)

5. `cargo run --bin recordit -- replay --jsonl /tmp/bd-2uuq-live/session.jsonl --format json`
- PASS
- replay envelope returned machine-readable payload (`event_count=36`)

## Happy-Path Assessment

Conclusion: **passes acceptance for operator walkthrough viability**.

What is clearly smaller now:
- top-level command vocabulary is reduced and discoverable from one help screen
- operators can complete preflight + offline + live + replay with a small stable command set
- output-root/session defaults remove manual artifact path assembly pressure

## Rough-Edge Cleanup List

1. `recordit preflight --mode live` currently yields `config.runtime_mode* = null` in preflight manifest output.
- Impact: machine consumers may expect explicit mode labels even for preflight.
- Severity: medium (machine-readability clarity), non-blocking for operator walkthrough.

2. `recordit run` still emits a large inherited `Transcribe-live configuration` block before runtime result.
- Impact: operator surface is simpler at command level but still verbose at runtime.
- Severity: medium (UX noise), non-blocking for correctness and artifact trust.

3. Live-run benchmark SLO line can report false in deterministic fixture runs even when session status is `ok`.
- Impact: may read as alarming to new operators without context.
- Severity: low-medium; mostly wording/positioning polish.

## Validation Outcome for Bead

`bd-2uuq` objective is satisfied: the new product surface has been exercised as a human workflow, rough edges are explicitly enumerated, and the happy path is operationally smaller at the command surface while preserving artifact/trust semantics.
