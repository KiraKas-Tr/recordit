# bd-d8lw Pending Sidecar Writer + Validation

Implemented pending-session sidecar contract and validation for deferred record-only workflows.

## Contract Surface

Added typed DTO + writer request in `app/Services/ServiceInterfaces.swift`:

- `PendingTranscriptionState` enum:
  - `pending_model`
  - `ready_to_transcribe`
  - `transcribing`
  - `completed`
  - `failed`
- `PendingSessionSidecarDTO`:
  - `session_id`
  - `created_at_utc`
  - `wav_path`
  - `mode`
  - `transcription_state`
- `PendingSessionSidecarWriteRequest`
- `PendingSessionSidecarService` protocol

## Filesystem Writer

`app/Services/PendingSessionSidecarService.swift` provides:

1. Canonical writer for `<session_root>/session.pending.json`.
2. Input validation:
   - non-empty `session_id`
   - absolute `session_root` and `wav_path`
   - `mode == record_only`
   - `wav_path` must be `<session_root>/session.wav`
3. JSON encoding with stable key ordering and atomic file write.
4. Read/validation API (`loadPendingSidecar`) for schema checks.

## Malformed Sidecar Diagnostics

`app/Services/ArtifactIntegrityService.swift` now emits recoverable finding:

- code: `pending_sidecar_invalid`
- summary: pending sidecar malformed
- diagnostics include pending sidecar path + parse/validation error

This ensures malformed `session.pending.json` files are surfaced for recovery without forcing terminal failure.
