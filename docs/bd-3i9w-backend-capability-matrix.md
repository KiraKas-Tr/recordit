# bd-3i9w: Backend Capability Matrix Enforcement (Model Setup)

## Goal

Ensure model setup only allows supported backends as selectable-ready options, blocks backend/model-path kind mismatch before runtime start, and provides plain-language remediation.

## Delivered

1. `app/AppShell/ModelSetupViewModel.swift`
2. `app/Services/model_resolution_smoke.swift`

## What Landed

1. Added explicit backend capability matrix in `ModelSetupViewModel` with typed metadata:
- backend id + display name
- selectable vs unsupported state
- required model-path kind (`file` or `directory`)
- per-backend remediation copy

2. Added unsupported backend representation (`moonshine`) as non-selectable:
- unsupported backends are retained in capability metadata
- unsupported backends are excluded from `selectableBackends`
- selecting unsupported backends yields deterministic `State.invalid(AppServiceError(.invalidInput))` and keeps current selectable backend unchanged

3. Added pre-start backend/path kind guard in the view-model:
- `whispercpp` requires a file path
- `whisperkit` requires a directory path
- mismatches fail early with plain-language guidance before launch enablement

4. Preserved existing diagnostics contract for valid selections:
- resolved model path/source/checksum fields still flow through `diagnostics`
- `canStartLiveTranscribe` remains tied to `.ready`

## Acceptance Mapping

1. Unsupported backends never selectable-ready in primary setup UX:
- enforced by `selectableBackends` derived from capability matrix
- unsupported backend selection path is blocked with clear remediation

2. Backend/model kind mismatch prevented pre-start:
- explicit path-kind validation runs before model resolution completion
- start gate remains disabled on mismatch (`State.invalid`)

3. Plain-language remediation:
- unsupported backend and path-kind mismatch errors use operator-facing, non-jargon copy

## Validation

1. Module compile/typecheck for model setup + service interfaces
2. Smoke runner updates verify:
- mismatch maps to `.invalidInput` with folder/file guidance
- unsupported backend does not become active selection
- unsupported backend absent from selectable list
