# Operator Path Acceptance Pass (`bd-2uuq`)

Date: 2026-03-02  
Status: final pass complete on current shared state after `bd-mqpf` closure

## Scope

This pass validates the primary operator path through `recordit` after Phase C/D/E changes:

1. first-run discoverability (`recordit --help`)
2. environment readiness (`recordit doctor`)
3. live preflight (`recordit preflight --mode live`)
4. offline happy path (`recordit run --mode offline`)
5. live happy path with deterministic fake capture (`recordit run --mode live`)
6. replay usability (`recordit replay --jsonl ... --format json`)

## Walkthrough Evidence

Inputs used:

- model: `artifacts/bench/models/whispercpp/ggml-tiny.en.bin`
- fixture: `artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav`
- output roots:
  - `/tmp/recordit-acceptance-preflight`
  - `/tmp/recordit-acceptance-offline`
  - `/tmp/recordit-acceptance-live`

Commands run:

```bash
cargo run --bin recordit -- --help
cargo run --bin recordit -- doctor --json
cargo run --bin recordit -- preflight --mode live --output-root /tmp/recordit-acceptance-preflight --json
cargo run --bin recordit -- run --mode offline --input-wav artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav --output-root /tmp/recordit-acceptance-offline --model artifacts/bench/models/whispercpp/ggml-tiny.en.bin --json
RECORDIT_FAKE_CAPTURE_FIXTURE=artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav RECORDIT_FAKE_CAPTURE_REALTIME=0 cargo run --bin recordit -- run --mode live --output-root /tmp/recordit-acceptance-live --model artifacts/bench/models/whispercpp/ggml-tiny.en.bin --json
cargo run --bin recordit -- replay --jsonl /tmp/recordit-acceptance-live/session.jsonl --format json
```

Observed results:

- `--help` clearly exposes the canonical operator verbs and mode model.
- `doctor --json` passed and returned deterministic machine-readable summary envelope.
- `preflight --mode live --json` passed and emitted manifest + summary envelope.
- offline run succeeded with deterministic startup banner + close summary.
- live run (fake capture) succeeded with deterministic startup banner + close summary.
- replay returned a machine-readable event envelope from generated JSONL.
- failure-path contract checks now pass for operator guidance:
  - parse failures emit `run_status=failed` with `remediation_hint=...`
  - replay failures emit `run_status=failed` with replay-specific `remediation_hint=...`
  - validated by `cargo test --test transcribe_live_failed_status_contract -- --nocapture`

## Remaining Rough Edges (Non-Blocking)

1. Default runtime output remains very verbose for normal operator usage.
   - Both offline and live `recordit run` paths still print full transcribe configuration + deep telemetry blocks before/after close summary.
   - This is truthful and useful for engineering, but it weakens the “smaller happy path” objective for first-time operators.
2. SLO signal communication is not yet tied to operator remediation severity.
   - In the live pass, `slo_check` printed false for both thresholds while `run_status=ok` and `remediation_hints=<none>`.
   - This may be acceptable by policy, but in operator UX terms it can feel contradictory without explicit framing.
3. Preflight and runtime summary surfaces use different field nesting conventions.
   - Preflight manifest reports runtime mode metadata under preflight config sections, while runtime manifests expose mode fields top-level.
   - This is contract-valid, but may still confuse operators reading mixed outputs without a compact crosswalk.

## Happy Path Assessment (Final)

- First-run path is operational and deterministic.
- Core operator commands (`help`, `doctor`, `preflight`, `run`, `replay`) succeed on realistic local inputs.
- Artifact outputs remain stable and mode-correct across offline/live runs.
- Run-status/remediation guidance is now explicit and deterministic for failure paths.

Final verdict: **pass, with follow-up UX simplification caveats**.

## Closure Recommendation

`bd-2uuq` acceptance criteria are met: the operator happy path is validated end-to-end in current shared state, and remaining rough edges are quality improvements rather than blockers.
