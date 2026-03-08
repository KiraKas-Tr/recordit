# bd-3drr: Deferred Queue and Pending Transition Integration Tests

## Scope

Added integration-style deferred queue coverage to lock the pending-session lifecycle contract:

1. `pending_model -> ready_to_transcribe -> transcribing -> completed`
2. `ready_to_transcribe -> transcribing -> failed`
3. successful manifest promotion removes pending sidecar artifacts
4. failed runs preserve retry context and failed sidecar state

## Implementation

### `app/Services/pending_queue_integration_smoke.swift`

Added a focused end-to-end smoke that composes existing services:

1. `FileSystemPendingSessionSidecarService`
2. `PendingSessionTransitionService`
3. `PendingSessionTranscriptionService`
4. `PendingSessionFinalizerService`

Test behavior:

1. success path:
   - starts from `pending_model`
   - reconciles readiness to `ready_to_transcribe`
   - verifies runtime observes `transcribing` sidecar state at launch time
   - verifies manifest-based completion and pending-sidecar removal
2. failure path:
   - manifest terminal status `failed`
   - verifies runtime still sees `transcribing` intermediate state
   - verifies final sidecar is `failed`
   - verifies `session.pending.retry.json` is persisted for retry flow

This test exercises the transition chain across service boundaries rather than isolated unit checks.

## Validation

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionFinalizerService.swift \
  app/Services/PendingSessionTranscriptionService.swift \
  -module-name RecordItPendingQueueIntegration \
  -o /tmp/RecordItPendingQueueIntegration.swiftmodule

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionFinalizerService.swift \
  app/Services/PendingSessionTranscriptionService.swift \
  app/Services/pending_queue_integration_smoke.swift \
  -o /tmp/pending_queue_integration_smoke && /tmp/pending_queue_integration_smoke

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionFinalizerService.swift \
  app/Services/PendingSessionTranscriptionService.swift \
  app/Services/pending_transcribe_action_smoke.swift \
  -o /tmp/pending_transcribe_action_smoke && /tmp/pending_transcribe_action_smoke

UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/Services/pending_queue_integration_smoke.swift \
  app/Services/PendingSessionTranscriptionService.swift \
  app/Services/PendingSessionFinalizerService.swift \
  docs/bd-3drr-integration-tests-pending-queue.md
```
