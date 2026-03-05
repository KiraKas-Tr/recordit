# bd-2n4m — Reliability gate: 10-session no-restart soak

## Summary
Implemented a deterministic 10-session soak gate that repeatedly exercises lifecycle finalization across both runtime lanes without restarting the test process:

- live start/stop/finalize loop (10 iterations)
- record-only deferred transcription finalize loop (10 iterations)

The soak asserts no orphaned live process tracking drift, stable finalization behavior, and manifest/sidecar artifact invariants per iteration.

## Delivered
- `app/Integration/process_lifecycle_soak_smoke.swift` (new)

## Gate Assertions

### Live lane (10 iterations)
- each iteration reaches `completed` after stop/finalization
- recovery action list remains empty on success
- active live process tracker returns to zero after each stop

### Record-only lane (10 iterations)
- each iteration returns pending action final state `completed`
- `session.pending.json` is removed after success
- retry context file is not present on success
- `session.manifest.json` exists and parses with `session_status=ok`
- active live process tracker remains zero during deferred lane runs

### End-of-gate counters
- `liveStarts == 10`
- `liveStops == 10`
- `offlineStarts == 10`
- `activeLiveProcessCount == 0`

## Validation
```bash
swiftc \
  app/Accessibility/AccessibilityContracts.swift \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionFinalizerService.swift \
  app/Services/PendingSessionTranscriptionService.swift \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  app/Integration/process_lifecycle_soak_smoke.swift \
  -o /tmp/process_lifecycle_soak_smoke && /tmp/process_lifecycle_soak_smoke
# process_lifecycle_soak_smoke: PASS
```

```bash
UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/Integration/process_lifecycle_soak_smoke.swift \
  docs/bd-2n4m-soak-gate.md
```
