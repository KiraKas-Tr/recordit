# bd-2jhd — Runtime session state machine transitions and guards

Implemented explicit runtime-view-model state transitions and invalid-action guards for start/stop/finalization flow.

## What Changed

1. Refined runtime run states:
- `idle`
- `preparing`
- `running(processID)`
- `stopping(processID)`
- `finalizing`
- `completed`
- `failed(AppServiceError)`

2. Added deterministic transition guardrail in `RuntimeViewModel`:
- transition helper enforces allowed predecessor phases for each state transition
- invalid transitions are rejected without mutating `state`
- rejections are surfaced via `lastRejectedActionError` (`AppServiceError(.invalidInput)`)

3. Start/stop guard behavior:
- `startLive` rejects concurrent or in-flight transition starts
- `stopCurrentRun` rejects stop requests unless actively `running`
- successful stop transitions to `finalizing` (not directly to `completed`)

4. Finalization consistency:
- `loadFinalStatus` performs explicit finalizing transition and then maps to `completed` or `failed`
- loading final status during `preparing` is rejected
- terminal state consistency preserved across stop/finalization/error paths

5. Added smoke coverage:
- `app/ViewModels/runtime_state_machine_smoke.swift`
  - idle stop rejection
  - concurrent start rejection while `preparing`
  - stop to `finalizing` transition
  - stop rejection during `finalizing`
  - final status mapping from `finalizing` to `completed`
  - rejection of final-status load during `preparing`

## Validation

- Module compile:
```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Accessibility/AccessibilityContracts.swift \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  -module-name RecordItRuntimeStateMachine \
  -o /tmp/RecordItRuntimeStateMachine.swiftmodule
```

- Smokes:
```bash
swiftc ... app/ViewModels/runtime_state_machine_smoke.swift -o /tmp/runtime_state_machine_smoke && /tmp/runtime_state_machine_smoke
# runtime_state_machine_smoke: PASS

swiftc ... app/ViewModels/runtime_status_mapping_smoke.swift -o /tmp/runtime_status_mapping_smoke && /tmp/runtime_status_mapping_smoke
# runtime_status_mapping_smoke: PASS
```

- UBS (scoped):
```bash
UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/ViewModels/RuntimeViewModel.swift \
  app/ViewModels/runtime_state_machine_smoke.swift \
  docs/bd-2jhd-runtime-state-machine.md
```
