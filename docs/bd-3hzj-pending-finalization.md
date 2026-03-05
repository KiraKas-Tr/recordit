# bd-3hzj Pending Finalization to Canonical Manifest Lifecycle

Implemented finalization step for successful deferred transcription runs so manifest becomes the canonical completion source and pending artifacts are cleaned up.

## Finalizer Service

`app/Services/PendingSessionFinalizerService.swift`

Behavior:

1. Requires canonical `session.manifest.json` to exist and parse with `session_summary.session_status`.
2. Removes `session.pending.json` on successful finalization.
3. Removes stale `session.pending.retry.json` when present.
4. Emits `AppServiceError(.manifestInvalid|.ioFailure)` for malformed manifest or cleanup failures.

## Pipeline Integration

`app/Services/PendingSessionTranscriptionService.swift`

On successful deferred transcription completion:

1. Transition to `completed`.
2. Invoke `PendingSessionFinalizerService.finalizePendingSession(...)`.
3. Return action result while leaving manifest as canonical status source.

Failure behavior remains unchanged:

- keeps pending sidecar with `failed` state
- persists retry context for user-facing retry flows.

## Acceptance Alignment

This ensures successful deferred sessions no longer leave stale pending artifacts and that status rendering can rely on canonical manifest lifecycle outputs.
