# bd-xknj Pending Discovery + Readiness Transitions

Implemented deterministic pending-session transition handling and surfaced readiness in discovery output.

## Transition State Machine

`app/Services/PendingSessionTransitionService.swift`

- legal transitions:
  - `pending_model -> ready_to_transcribe` on `model_available`
  - `ready_to_transcribe -> pending_model` on `model_unavailable`
  - `ready_to_transcribe -> transcribing` on `transcription_started`
  - `transcribing -> completed` on `transcription_completed`
  - `transcribing -> failed` on `transcription_failed`
  - `failed -> ready_to_transcribe` on `model_available` (retry lane)
- illegal jumps throw `AppServiceError(code: .invalidInput, ...)`.

## Discovery Integration

`app/Services/FileSystemSessionLibraryService.swift`

1. Session summary now exposes:
   - `pendingTranscriptionState`
   - `readyToTranscribe`
2. Pending-sidecar discovery uses `PendingSessionTransitionService.reconcileReadiness(...)` with model availability condition.
3. If readiness changes are detected during discovery, `session.pending.json` is atomically rewritten with the reconciled state.
4. Pending state maps into session status deterministically:
   - `pending_model|ready_to_transcribe|transcribing` -> `pending`
   - `completed` -> `ok`
   - `failed` -> `failed`

## Smoke Coverage

`app/Services/pending_transition_smoke.swift` covers:

1. legal transition chain: `pending_model -> ready_to_transcribe -> transcribing -> completed`
2. illegal jump rejection: `pending_model -> transcription_started`
3. discovery readiness reconciliation:
   - model available transitions persisted sidecar to `ready_to_transcribe`
   - model unavailable transitions persisted sidecar back to `pending_model`
