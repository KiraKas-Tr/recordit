# bd-2hxr: Export UX Flows and Naming Conventions

## Scope

Implemented explicit export UX flow state and deterministic filename preview behavior for:

1. transcript
2. audio
3. bundle
4. diagnostics

Also made diagnostics privacy toggles explicit in the view-model contract.

## Implementation

### `app/Exports/SessionExportViewModel.swift` (new)

Added a dedicated export UX state machine:

1. `FlowState`:
   - `idle`
   - `exporting(kind)`
   - `succeeded(result)`
   - `failed(error)`
2. deterministic filename previews per export kind:
   - `recordit-transcript-<id>.txt`
   - `recordit-audio-<id>.wav`
   - `recordit-session-<id>.zip`
   - `recordit-diagnostics-<id>.zip`
3. explicit diagnostics privacy controls:
   - `includeTranscriptTextInDiagnostics`
   - `includeAudioInDiagnostics`
   - toggles are automatically reset when leaving diagnostics mode
4. completion/error feedback surfaces:
   - `completionMessage`
   - `errorMessage`

### `app/Exports/session_export_view_model_smoke.swift` (new)

Added focused UX-flow assertions:

1. filename preview conventions for all export kinds
2. diagnostics toggle visibility + request wiring
3. success feedback semantics
4. failure feedback semantics with preserved `AppServiceError` mapping

## Validation

```bash
swiftc app/Services/ServiceInterfaces.swift app/Accessibility/AccessibilityContracts.swift app/Exports/SessionExportService.swift app/Exports/SessionExportViewModel.swift app/Exports/session_export_view_model_smoke.swift -o /tmp/session_export_view_model_smoke && /tmp/session_export_view_model_smoke

swiftc app/Services/ServiceInterfaces.swift app/Accessibility/AccessibilityContracts.swift app/Exports/SessionExportService.swift app/Exports/export_smoke.swift -o /tmp/export_smoke && /tmp/export_smoke

swiftc -parse-as-library -emit-module app/Services/ServiceInterfaces.swift app/Accessibility/AccessibilityContracts.swift app/Exports/SessionExportService.swift app/Exports/SessionExportViewModel.swift -module-name RecordItExportUX -o /tmp/RecordItExportUX.swiftmodule

UBS_MAX_DIR_SIZE_MB=5000 ubs app/Exports/SessionExportViewModel.swift app/Exports/session_export_view_model_smoke.swift docs/bd-2hxr-export-ux-flows.md
```
