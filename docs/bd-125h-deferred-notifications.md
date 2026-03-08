# bd-125h: Deferred Transcription Notifications and Actionable Completion States

## Scope

Implemented transition-driven deferred-session notifications with deep-link/retry semantics:

1. notify when deferred session becomes ready to transcribe
2. notify when deferred session completes/finalizes
3. notify when deferred session fails with explicit retry affordance

## Implementation

### `app/Services/PendingSessionNotificationService.swift` (new)

Added:

1. `PendingSessionNotificationKind` (`ready_to_transcribe`, `completed`, `failed`)
2. `PendingSessionNotificationAction`
   - `openSessionDetail(sessionID:)`
   - `retryDeferredTranscription(sessionID:)`
3. `PendingSessionNotificationIntent`
   - includes `deepLinkSessionID` for deterministic session-detail routing
4. `PendingSessionNotificationService`
   - compares previous vs current session summaries
   - emits notification intents on state transitions:
     - pending-model -> ready-to-transcribe
     - pending/transcribing -> completed (manifest-finalized session)
     - non-failed -> failed

### `app/ViewModels/SessionListViewModel.swift`

Integrated notification detection into refresh flow:

1. inject `PendingSessionNotificationDetecting` dependency
2. detect transitions each successful refresh using prior snapshot vs current snapshot
3. queue notifications in `pendingNotifications`
4. expose `consumePendingNotifications()` for UI delivery

### `app/ViewModels/session_list_smoke.swift`

Extended smoke coverage with sequenced snapshots to validate:

1. ready + failed notification emission on transition refresh
2. failed notification includes:
   - primary action: retry deferred transcription
   - secondary action: open session detail
3. completion notification emits after finalized state transition and includes deep-link target

## Validation

```bash
swiftc app/Services/ServiceInterfaces.swift app/Accessibility/AccessibilityContracts.swift app/Services/PendingSessionTransitionService.swift app/Services/PendingSessionSidecarService.swift app/Services/PendingSessionFinalizerService.swift app/Services/PendingSessionTranscriptionService.swift app/Services/PendingSessionNotificationService.swift app/ViewModels/SessionListViewModel.swift app/Services/MockServices.swift app/ViewModels/session_list_smoke.swift -o /tmp/session_list_smoke && /tmp/session_list_smoke

swiftc -parse-as-library -emit-module app/Services/ServiceInterfaces.swift app/Accessibility/AccessibilityContracts.swift app/Services/PendingSessionNotificationService.swift app/Services/PendingSessionTransitionService.swift app/Services/PendingSessionSidecarService.swift app/Services/PendingSessionFinalizerService.swift app/Services/PendingSessionTranscriptionService.swift app/ViewModels/SessionListViewModel.swift -module-name RecordItPendingNotification -o /tmp/RecordItPendingNotification.swiftmodule

UBS_MAX_DIR_SIZE_MB=5000 ubs app/Services/PendingSessionNotificationService.swift app/ViewModels/SessionListViewModel.swift app/ViewModels/session_list_smoke.swift docs/bd-125h-deferred-notifications.md
```
