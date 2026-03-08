# bd-7uap: Preflight Blocking/Warn Gating Contract

## Goal

Enforce deterministic preflight gating for onboarding `Live Transcribe` decisions from typed preflight checks, not from loosely interpreted summary text.

## Contract Mapping

`PreflightGatingPolicy` maps check IDs to gate behavior:

1. `blockOnFail`
- `model_path`
- `out_wav`
- `out_jsonl`
- `out_manifest`
- `screen_capture_access`
- `display_availability`
- `microphone_access`

2. `warnRequiresAcknowledgement`
- `sample_rate`
- `backend_runtime`

3. `informational`
- any non-contract ID not listed above

## Runtime UX Semantics

1. Any `blockOnFail` check with `FAIL` blocks `Live Transcribe`.
2. Any `warnRequiresAcknowledgement` check with non-`PASS` status requires explicit user acknowledgment before proceeding.
3. If no blocking failures and no unacknowledged warnings remain, proceed is allowed.

`PreflightViewModel` now exposes:

- `gatingEvaluation`
- `requiresWarningAcknowledgement`
- `canProceedToLiveTranscribe`
- `acknowledgeWarningsForLiveTranscribe()`

## Drift Guard

`preflight_gating_smoke.swift` asserts:

1. known contract check ID set is exact
2. policy mapping for each known ID is exact
3. blocking and warn-ack behavior gates proceed decisions correctly
4. view-model flow requires explicit warning acknowledgment before proceed
