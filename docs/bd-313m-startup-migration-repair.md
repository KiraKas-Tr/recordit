# bd-313m: Startup Migration/Index Repair Job

## Scope

Implemented a bounded, best-effort startup repair lane for session-library migration/index hygiene:

1. reconcile stale and missing session index entries against current library scan results
2. include legacy-import counts in startup repair diagnostics
3. keep failures non-fatal and log-safe
4. preserve queryability of the session library after repair attempts

## Implementation

### `app/Services/StartupMigrationRepairService.swift`

Added:

1. `StartupMigrationRepairReport`
   - captures scan counts, stale/missing reconciliation counts, legacy import counts, truncation, time-budget status, and failure diagnostics.
2. `StartupMigrationRepairing` protocol
   - small abstraction for app bootstrap wiring.
3. `StartupMigrationRepairService`
   - non-throwing `runRepair()` flow.
   - bounded by `timeBudgetSeconds` and `maxPersistedEntries`.
   - reads existing persisted startup index (`.recordit/session-library-index.json`) if present.
   - rewrites index atomically to remove stale entries and add missing current entries.
   - performs a post-repair queryability check via `SessionLibraryService.listSessions(...)`.
   - logs summary/failure diagnostics without surfacing fatal startup errors.

### `app/AppShell/AppEnvironment.swift`

Wired startup repair into the DI environment:

1. new optional dependency: `startupMigrationRepairService`
2. production wiring now provides `StartupMigrationRepairService` with the filesystem-backed session library service
3. preview wiring includes a bounded startup repair service using preview-safe temp-root resolution and silent logger
4. added APIs:
   - `runStartupMigrationRepair()`
   - `scheduleStartupMigrationRepair(priority:)`

### `app/AppShell/app_environment_smoke.swift`

Extended smoke coverage to verify:

1. startup repair service can be injected through `AppEnvironment.replacing(...)`
2. synchronous and asynchronous startup repair execution surfaces deterministic reports

### `app/Services/startup_migration_repair_smoke.swift`

Added focused repair smoke coverage:

1. stale/missing index reconciliation with persisted index input
2. legacy import counting and post-repair queryability assertions
3. non-fatal failure path behavior when session listing fails

## Validation

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Services/MockServices.swift \
  app/Services/StartupMigrationRepairService.swift \
  app/AppShell/AppEnvironment.swift \
  app/AppShell/AppShellViewModel.swift \
  app/AppShell/ModelSetupViewModel.swift \
  app/AppShell/PermissionRemediationViewModel.swift \
  app/AppShell/PreflightViewModel.swift \
  app/AppShell/OnboardingCompletionStore.swift \
  app/Navigation/NavigationModels.swift \
  app/Navigation/AppNavigationCoordinator.swift \
  app/Preflight/PreflightRunner.swift \
  app/Services/FileSystemJsonlTailService.swift \
  app/Services/FileSystemModelResolutionService.swift \
  app/Services/FileSystemSessionLibraryService.swift \
  app/Services/ArtifactIntegrityService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/SessionTranscriptSearchIndex.swift \
  -module-name RecordItStartupRepair \
  -o /tmp/RecordItStartupRepair.swiftmodule

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/MockServices.swift \
  app/Services/StartupMigrationRepairService.swift \
  app/Services/startup_migration_repair_smoke.swift \
  -o /tmp/startup_migration_repair_smoke && /tmp/startup_migration_repair_smoke

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/MockServices.swift \
  app/Services/StartupMigrationRepairService.swift \
  app/AppShell/OnboardingCompletionStore.swift \
  app/AppShell/AppShellViewModel.swift \
  app/AppShell/AppEnvironment.swift \
  app/AppShell/ModelSetupViewModel.swift \
  app/AppShell/PermissionRemediationViewModel.swift \
  app/AppShell/PreflightViewModel.swift \
  app/Navigation/NavigationModels.swift \
  app/Navigation/AppNavigationCoordinator.swift \
  app/Preflight/PreflightRunner.swift \
  app/Services/FileSystemJsonlTailService.swift \
  app/Services/FileSystemModelResolutionService.swift \
  app/Services/FileSystemSessionLibraryService.swift \
  app/Services/ArtifactIntegrityService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/SessionTranscriptSearchIndex.swift \
  app/AppShell/app_environment_smoke.swift \
  -o /tmp/app_environment_smoke && /tmp/app_environment_smoke

UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/Services/StartupMigrationRepairService.swift \
  app/Services/startup_migration_repair_smoke.swift \
  app/AppShell/AppEnvironment.swift \
  app/AppShell/app_environment_smoke.swift \
  docs/bd-313m-startup-migration-repair.md
```
