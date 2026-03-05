# bd-33d5: Model Setup Existing-Path Validation

## Goal

Implement model setup behavior that lets users choose an existing local model path and blocks invalid selections before runtime start, while surfacing source/checksum diagnostics.

## Delivered

1. `app/Services/FileSystemModelResolutionService.swift`
2. `app/AppShell/ModelSetupViewModel.swift`
3. `app/Services/model_resolution_smoke.swift`

## Service Behavior

`FileSystemModelResolutionService` implements `ModelResolutionService` with deterministic resolution order:

1. explicit UI-selected path (`explicitModelPath`)
2. `RECORDIT_ASR_MODEL`
3. backend defaults

Supported backends in this lane:

1. `whispercpp` (expects file path)
2. `whisperkit` (expects directory path)

Validation and diagnostics:

1. missing path => `AppServiceError(.modelUnavailable)`
2. backend-kind mismatch => `AppServiceError(.modelUnavailable)` with remediation
3. returned `ResolvedModelDTO` includes:
- `resolvedPath`
- `source`
- `checksumSHA256` (when available)
- `checksumStatus`

Checksum status mapping:

1. `available`
2. `unavailable_directory`
3. `unavailable_not_file`
4. `unavailable_unresolved`
5. `unavailable_checksum_error`

## View-model Behavior

`ModelSetupViewModel` provides onboarding-facing state and gating:

1. backend selection (`whispercpp` / `whisperkit`)
2. existing path selection
3. immediate validation via `ModelResolutionService`
4. `canStartLiveTranscribe` gate
5. surfaced diagnostics:
- `asrModel`
- `asrModelSource`
- `asrModelChecksumSHA256`
- `asrModelChecksumStatus`

Invalid selections produce `State.invalid(AppServiceError)` so UI can display clear remediation and keep live start disabled.
