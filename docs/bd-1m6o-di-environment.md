# bd-1m6o: DI Container and App Environment Wiring

## Goal

Provide explicit dependency wiring for production and preview/test contexts so:

1. service construction is centralized
2. preview/test runs can use doubles without runtime subprocess spawn
3. replacement points are explicit and easy to override in tests

## Delivered

1. `app/AppShell/AppEnvironment.swift`
2. `app/AppShell/app_environment_smoke.swift`

## Container Design

`AppEnvironment` stores service dependencies as protocol-backed fields:

1. `runtimeService`
2. `manifestService`
3. `modelService`
4. `jsonlTailService`
5. `sessionLibraryService`
6. `artifactIntegrityService`
7. `pendingSidecarService`
8. `preflightRunner`

## Wiring Modes

1. `AppEnvironment.production()`
- process-backed runtime service
- filesystem-backed manifest/model/jsonl/library/integrity services
- real preflight runner

2. `AppEnvironment.preview()`
- mock runtime/model/manifest/library/integrity services
- fixture-backed preflight command runner
- no external runtime process spawn required

## Override Points

`replacing(...)` enables per-service overrides for tests and previews without rewriting global construction.

## View-model Factories

The environment provides centralized factory methods:

1. `makeRuntimeViewModel()`
2. `makePreflightViewModel()`
3. `makePermissionRemediationViewModel(...)`
4. `makeModelSetupViewModel()`

## Validation

`app_environment_smoke.swift` verifies:

1. preview environment runtime path uses mock runtime (no subprocess dependency)
2. service override (`modelService`) changes runtime outcome deterministically
3. preview preflight view-model completes from fixture payload
