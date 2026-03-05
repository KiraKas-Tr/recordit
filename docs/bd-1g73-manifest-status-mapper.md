# bd-1g73 Manifest-Driven Final Status Mapping

## Scope

Implemented manifest-driven final status mapping so final run state is derived from manifest semantics (`session_summary.status` + trust notice count), not terminal text.

## Implementation

### `app/RuntimeStatus/ManifestFinalStatusMapper.swift`

Added `ManifestFinalStatusMapper`:
- input: `SessionManifestDTO`
- output: `SessionStatus`
- mapping rules:
  - `failed` -> `.failed`
  - `degraded` -> `.degraded`
  - `pending` -> `.pending`
  - `ok` with `trustNoticeCount > 0` -> `.degraded`
  - `ok` with zero trust notices -> `.ok`
  - unknown status strings fallback to trust-aware interpretation (`.degraded` when trust notices exist, else `.ok`)

This preserves degraded-success semantics and prevents false failure presentation.

### `app/ViewModels/RuntimeViewModel.swift`

Updated final status loading:
- added injected `finalStatusMapper` dependency (defaulting to `ManifestFinalStatusMapper()`).
- `loadFinalStatus(manifestPath:)` now:
  1. loads manifest via `ManifestService`
  2. maps status via mapper
  3. maps `.failed` to `.failed(AppServiceError(.processExitedUnexpectedly))`
  4. maps `.ok`/`.degraded`/`.pending` to `.completed`

This ensures degraded completion is never represented as failed.

### `app/ViewModels/runtime_status_mapping_smoke.swift`

Added focused smoke checks for:
- mapper branches: `ok`, `degraded`, `failed`, trust-driven degraded
- runtime VM behavior:
  - `ok` -> `.completed`
  - trust-degraded success -> `.completed` (not failed)
  - `failed` -> `.failed` with expected error code

## Validation

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Accessibility/AccessibilityContracts.swift \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  -module-name RecordItManifestStatusMapper \
  -o /tmp/RecordItManifestStatusMapper.swiftmodule

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Accessibility/AccessibilityContracts.swift \
  app/Services/MockServices.swift \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  app/ViewModels/runtime_status_mapping_smoke.swift \
  -o /tmp/runtime_status_mapping_smoke && /tmp/runtime_status_mapping_smoke

UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/RuntimeStatus/ManifestFinalStatusMapper.swift \
  app/ViewModels/RuntimeViewModel.swift \
  app/ViewModels/runtime_status_mapping_smoke.swift \
  docs/bd-1g73-manifest-status-mapper.md
```
