# bd-1m3z Transcribe Pending Session Action Pipeline

Implemented deferred offline transcription action flow for ready pending sessions.

## Action Service

`app/Services/PendingSessionTranscriptionService.swift`

Primary behavior:

1. Accepts only ready record-only items:
   - requires `mode == record_only`
   - requires `pendingTranscriptionState == ready_to_transcribe`
   - requires `readyToTranscribe == true`
2. Loads pending sidecar and transitions atomically:
   - `ready_to_transcribe -> transcribing`
   - terminal transition to `completed` or `failed`
3. Executes offline transcription via runtime service:
   - `recordit run --mode offline` through existing `RuntimeService` abstraction
4. Waits for manifest completion with bounded timeout.
5. Preserves retry context on failure:
   - writes `<session_root>/session.pending.retry.json` with failure timestamp/message
   - persists sidecar `transcription_state = failed`

## Session List Wiring

`app/ViewModels/SessionListViewModel.swift`

Added async action:

- `transcribePendingSession(sessionID:)`

Behavior:

1. Rejects non-ready actions with `AppServiceError(code: .invalidInput)` and keeps recoverable items visible.
2. Uses `PendingSessionTranscribing` service abstraction.
3. Refreshes list after successful action dispatch/completion.

## Smoke Coverage

1. `app/Services/pending_transcribe_action_smoke.swift`
   - success path (`ready -> transcribing -> completed`)
   - failure path preserves retry context and `failed` sidecar state
2. `app/ViewModels/session_list_smoke.swift`
   - verifies view-model triggers action only for ready sessions
   - verifies non-ready action rejection preserves recoverable list content
