# Packaged Live-Stream Argument and Artifact Plumbing Plan (bd-31s)

Date: 2026-03-01
Status: design contract for post-v1 packaged live-stream implementation

## Superseded Entrypoint Context (2026-03-05)

Canonical user-facing default is `Recordit.app` per
`docs/adr-005-recordit-default-entrypoint.md`.

This document is retained as historical packaging-plumbing guidance for legacy
`SequoiaTranscribe.app` compatibility/fallback lanes and should not be read as the
current product-default entrypoint policy.

## Goal

Define deterministic signed-app argument plumbing and container artifact destinations for live-stream mode so downstream packaged tasks (`bd-3ma`, `bd-3dx`) can implement diagnostics and smoke gates without re-deciding naming or path semantics.

## Scope

This plan covers packaged transcribe entrypoint plumbing only:

- signed app target naming
- argument composition rules
- artifact destination naming in container storage
- compatibility guardrails against representative-mode regressions

Out of scope for this bead:

- implementing packaged live-stream preflight/model-doctor behavior parity (handled by `bd-3ma`)
- adding packaged live smoke/gate execution flow (handled by `bd-3dx`)

## Baseline (current behavior)

Historical packaged default entrypoint (now non-default) is:

- `make run-transcribe-app`

It launches signed `SequoiaTranscribe.app` and writes session artifacts under:

- `$(TRANSCRIBE_APP_ARTIFACT_ROOT)`
- `$(TRANSCRIBE_APP_SESSION_STEM).wav`
- `$(TRANSCRIBE_APP_SESSION_STEM).jsonl`
- `$(TRANSCRIBE_APP_SESSION_STEM).manifest.json`

Runtime selection is currently passed through via `TRANSCRIBE_ARGS`.

## Planned target plumbing contract

### 1) Dedicated packaged live runtime target

Introduce a dedicated wrapper target for explicit live-stream intent:

- `run-transcribe-live-stream-app`

Contract:

- must invoke the same signed app path as `run-transcribe-app`
- must append `--live-stream` explicitly (no implicit selector behavior)
- must provide an explicit packaged input path via `--input-wav`
- may preserve user pass-through args via a dedicated live-stream suffix variable

### 2) Dedicated packaged diagnostics target names

Reserve the following names for parity work (implemented in `bd-3ma`):

- `run-transcribe-live-stream-preflight-app`
- `run-transcribe-live-stream-model-doctor-app`

Contract:

- names must mirror existing representative packaged diagnostics pattern
- diagnostics output must remain container-scoped and machine-readable
- until compatibility is implemented, these names should be treated as planned (not silently aliased)

## Planned variable contract (additive)

Use additive variables (no breaking rename of existing packaged defaults):

- `TRANSCRIBE_APP_LIVE_STREAM_INPUT_WAV`
  - default: `$(TRANSCRIBE_APP_ARTIFACT_ROOT)/$(TRANSCRIBE_APP_SESSION_STEM).input.wav`
- `TRANSCRIBE_APP_LIVE_STREAM_ARGS`
  - pass-through extra args for live-stream packaged runs

Recommended live-stream artifact stems (to avoid ambiguity during mixed representative/live workflows):

- input capture path: `<stem>.input.wav`
- runtime outputs remain canonical session outputs unless explicitly split by downstream implementation:
  - `<stem>.wav`
  - `<stem>.jsonl`
  - `<stem>.manifest.json`

If downstream work chooses split stems (`<stem>.live.*`), it must remain additive and documented in runbook/README.

## Container artifact destination invariants

For packaged live-stream runs, destination semantics must remain:

1. absolute container-scoped paths only
2. pre-run path printout before launch
3. post-run manifest existence summary at the same path
4. manifest `runtime_mode`/taxonomy fields remain authoritative for mode interpretation

Canonical root remains:

- `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/`

## Compatibility guardrails

1. In legacy compatibility lanes, `run-transcribe-app` remains representative unless operator explicitly selects live-stream wrapper.
2. In product-default release guidance, `Recordit.app` remains canonical (ADR-005).
3. Argument plumbing must stay additive; existing environment overrides remain valid.
4. No representative-mode semantic changes in packaged diagnostics while live-stream parity is pending.
5. Live-stream wrapper must preserve existing trust/degradation summary print contract.

## Downstream handoff

### For `bd-3ma` (diagnostics parity)

Implement planned live diagnostics targets with explicit compatibility behavior and update:

- `Makefile`
- `README.md`
- `docs/transcribe-operator-runbook.md`

Acceptance expectation:

- live-stream diagnostic invocations are explicit and produce machine-readable packaged artifacts without breaking representative diagnostics.

### For `bd-3dx` (packaged smoke/gates)

Use this naming/path contract when adding packaged live smoke/gate flows so artifact roots and command shape remain consistent with runtime + diagnostics targets.

## Verification checklist for future implementation

1. `make help` lists packaged live runtime/diagnostic targets clearly.
2. packaged live runtime prints explicit input/output artifact paths.
3. generated manifest confirms `runtime_mode=live-stream` when live wrapper is used.
4. representative packaged path remains unchanged unless operator chooses live wrapper.
