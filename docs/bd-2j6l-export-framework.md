# bd-2j6l: Session Export Framework

Date: 2026-03-05

## Scope

Implemented a file-system-backed export framework with typed request/result models for:

1. transcript export
2. audio export
3. bundle export
4. diagnostics export

Code location: `app/Exports/SessionExportService.swift`

## Contracts

### Request Model

`SessionExportRequest` fields:

1. `sessionID`
2. `sessionRoot`
3. `outputDirectory`
4. `kind` (`transcript|audio|bundle|diagnostics`)
5. `includeTranscriptTextInDiagnostics` (default `false`)
6. `includeAudioInDiagnostics` (default `false`)

### Result Model

`SessionExportResult` fields:

1. `kind`
2. `outputURL`
3. `exportedAt`
4. `includedArtifacts`
5. `redacted`

## Deterministic Output Naming

Given normalized `<session_id>`:

1. transcript: `recordit-transcript-<session_id>.txt`
2. audio: `recordit-audio-<session_id>.wav`
3. bundle: `recordit-session-<session_id>.zip`
4. diagnostics: `recordit-diagnostics-<session_id>.zip`

Session identifiers are slug-normalized to alphanumeric/`-`/`_` to keep filenames stable and portable.

## Privacy Defaults and Redaction

Diagnostics exports are redacted by default:

1. transcript-bearing fields are scrubbed unless `includeTranscriptTextInDiagnostics=true`
2. `session.wav` is excluded unless `includeAudioInDiagnostics=true`
3. redacted exports include `diagnostics.json` metadata describing included artifacts and opt-in flags

## Path Policy Enforcement

When `RECORDIT_ENFORCE_APP_MANAGED_STORAGE_POLICY` is enabled (`1|true|yes|on`):

1. `sessionRoot` must be inside canonical sessions root
2. `outputDirectory` must be inside canonical sessions root
3. canonical sessions root resolves via existing app contract:
   - `RECORDIT_CONTAINER_DATA_ROOT` override (absolute)
   - otherwise `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/sessions`

Exports outside policy roots fail with `AppServiceError(code: .permissionDenied)`.

## Corruption Safety

Export operations are non-destructive to source session artifacts:

1. source artifacts are read/copied only
2. outputs are written via temp staging + atomic replacement/move
3. archive exports are built in temporary staging directories then atomically promoted

## Validation

Smoke harness: `app/Exports/export_smoke.swift`

Covers:

1. deterministic filenames for all export kinds
2. transcript export preference for manifest stable lines
3. diagnostics redaction behavior by default
4. policy rejection for destinations outside managed sessions root
