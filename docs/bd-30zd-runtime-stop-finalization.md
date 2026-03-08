# bd-30zd — Graceful stop/finalization UX and failure recovery actions

Implemented bounded stop/finalization behavior and explicit recovery-action classification in the runtime view model.

## What Changed

1. Added recovery-action surface in `RuntimeViewModel`:
- `RecoveryAction` enum:
  - `retry_stop`
  - `retry_finalize`
  - `open_session_artifacts`
  - `run_preflight`
  - `start_new_session`
- `suggestedRecoveryActions` state to expose deterministic next actions after failures.

2. Added bounded stop-finalization flow:
- `stopCurrentRun()` now transitions:
  - `running -> stopping -> finalizing`
- On successful stop control, bounded finalization starts automatically (`finalizeStopBounded`).
- Finalization polls for `session.manifest.json` under active session root until timeout.
- Finalization always exits to terminal state (`completed` or `failed`) so no stuck transition phase.

3. Added active-session root tracking:
- launch success persists `activeSessionRoot` for stop-time finalization path resolution.

4. Added failure classification with explicit recovery mappings:
- timeout finalization -> `[retry_finalize, open_session_artifacts, start_new_session]`
- failed manifest status -> `[open_session_artifacts, start_new_session]`
- stop control failures -> `[retry_stop, open_session_artifacts]`
- startup/runtime/preflight lane failures -> mapped to deterministic action sets.

5. Preserved and extended transition guards:
- invalid/concurrent action rejections continue to populate `lastRejectedActionError` without mutating state.

6. Added/updated smoke coverage:
- `app/ViewModels/runtime_state_machine_smoke.swift` updated for bounded finalization completion semantics.
- `app/ViewModels/runtime_stop_finalization_smoke.swift` (new):
  - transient missing-manifest retries then success
  - finalization timeout classification and recovery actions
  - failed manifest-status classification and recovery actions

## Validation

- Module compile:
```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Accessibility/AccessibilityContracts.swift \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  -module-name RecordItRuntimeFinalization \
  -o /tmp/RecordItRuntimeFinalization.swiftmodule
```

- Smokes:
```bash
swiftc ... app/ViewModels/runtime_state_machine_smoke.swift -o /tmp/runtime_state_machine_smoke && /tmp/runtime_state_machine_smoke
# runtime_state_machine_smoke: PASS

swiftc ... app/ViewModels/runtime_stop_finalization_smoke.swift -o /tmp/runtime_stop_finalization_smoke && /tmp/runtime_stop_finalization_smoke
# runtime_stop_finalization_smoke: PASS

swiftc ... app/ViewModels/runtime_status_mapping_smoke.swift -o /tmp/runtime_status_mapping_smoke && /tmp/runtime_status_mapping_smoke
# runtime_status_mapping_smoke: PASS
```

- UBS (scoped):
```bash
UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/ViewModels/RuntimeViewModel.swift \
  app/ViewModels/runtime_state_machine_smoke.swift \
  app/ViewModels/runtime_stop_finalization_smoke.swift \
  docs/bd-30zd-runtime-stop-finalization.md
```
