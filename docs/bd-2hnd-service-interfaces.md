# bd-2hnd Service Interface Scaffold

This document defines the Wave-0 service boundary scaffold for the app shell.

## Delivered Files

- `app/Services/ServiceInterfaces.swift`
- `app/Services/MockServices.swift`
- `app/ViewModels/RuntimeViewModel.swift`

## Interface Coverage

Protocols are defined for the required service lanes:

1. `RuntimeService`
2. `JsonlTailService`
3. `ManifestService`
4. `ModelResolutionService`
5. `SessionLibraryService`

Boundary DTOs are centralized in `ServiceInterfaces.swift` and include:

- session runtime request/result shapes
- JSONL cursor + event DTOs
- manifest/session artifact DTOs
- model resolution DTOs
- session library query/summary DTOs
- shared `AppServiceError` taxonomy with user-facing remediation mapping

## Mockability

`MockServices.swift` provides mock implementations for each service protocol so view-model and coordinator layers can be tested without invoking subprocesses or filesystem decoding logic.

## ViewModel Boundary Check

`RuntimeViewModel` depends only on protocol types (`RuntimeService`, `ManifestService`, `ModelResolutionService`) and does not launch processes directly. This preserves the boundary rule: view-model layer consumes service interfaces only.

## Compile Check Command

Use this command to verify the scaffold compiles:

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Services/MockServices.swift \
  app/ViewModels/RuntimeViewModel.swift \
  -module-name RecordItAppScaffold \
  -o /tmp/RecordItAppScaffold.swiftmodule
```
