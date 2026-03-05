# SwiftUI Module Ownership Map and Dependency Boundaries

Bead: `bd-2zxu`  
Last updated: 2026-03-05

This document defines the canonical ownership boundaries for the macOS SwiftUI app shell described in `plan/recordit-user-interfaces-journey.md`.

## Goal

Keep UI code decoupled from runtime process internals so teams can work in parallel and we can test logic without launching real subprocesses from view code.

## Module Ownership Map

| Module | Owns | Must Not Own |
|---|---|---|
| `AppShell` | App/window lifecycle, root composition, route selection (onboarding vs runtime vs library) | Runtime process control details, transcript decoding logic |
| `OnboardingCoordinator` | Permission + model readiness flow, onboarding routing decisions | Direct process spawning, direct JSONL parsing |
| `SessionCoordinator` | Runtime screen flow state, start/stop orchestration decisions, session-level error routing | Low-level `Process` setup, filesystem scanning/parsing details |
| `ViewModels` | View-facing state shaping, user intents, presentation-ready status/copy | Spawning binaries, filesystem mutations, schema parsing |
| `Services` | Runtime process management, JSONL tail/decode, manifest read/validate, model resolution, session library indexing | SwiftUI view composition, direct navigation decisions |
| `RuntimeProcessLayer` | Command construction, subprocess supervision, exit/timeout classification | Any import of SwiftUI/Combine view state types |

## Allowed Dependency Directions

Dependency flow is top-down only:

1. `AppShell` -> `OnboardingCoordinator`, `SessionCoordinator`
2. `OnboardingCoordinator` -> onboarding-focused `ViewModels` + `Services`
3. `SessionCoordinator` -> runtime/library `ViewModels` + `Services`
4. `ViewModels` -> `Services` via protocols (or coordinator-owned interfaces)
5. `Services` -> `RuntimeProcessLayer` + contract models/utilities
6. `RuntimeProcessLayer` -> system/runtime binaries (`recordit`, `sequoia_capture`)

Rules:

1. Coordinators are the only layer allowed to drive navigation and workflow transitions.
2. Runtime process start/stop is allowed only through `RuntimeProcessManager` in the service/process layer.
3. UI reads runtime outputs through service abstractions (JSONL/manifest resolvers), not direct file/process access from Views.

## Forbidden Dependency Directions (Anti-Patterns)

1. `View`/`ViewModel` -> `Process` spawning or shell command execution.
2. `View`/`ViewModel` -> direct reads/writes of `session.jsonl`/`session.manifest.json`.
3. `View`/`ViewModel` -> direct model-path resolution filesystem logic.
4. `RuntimeProcessLayer` -> any `SwiftUI` or app-navigation type.
5. `Services` -> importing concrete View types (service-to-view coupling).
6. Cross-coordinator direct calls that bypass shared service interfaces for runtime/session state.

## PR Review Checklist (Boundary Guard)

Use this checklist for any PR touching app-shell architecture:

1. No `View` or `ViewModel` launches subprocesses directly.
2. No `View` or `ViewModel` decodes JSONL/manifest schemas directly.
3. Process execution changes are isolated to service/process modules.
4. New UI features consume runtime/session data through interfaces, not filesystem calls from views.
5. New services do not import SwiftUI symbols.
6. Navigation/state transitions remain coordinator-owned.
7. Any necessary boundary exception is documented in the PR with rationale and follow-up bead to remove it.

## Suggested Quick Audit Commands

Use these lightweight checks during review:

```bash
rg -n "Process\\(|Command\\(" app/ --glob "*.swift"
rg -n "session\\.jsonl|session\\.manifest\\.json" app/ --glob "*.swift"
rg -n "import SwiftUI" app/Services app/RuntimeProcessLayer --glob "*.swift"
```

If these are non-empty, verify they align with the allowed ownership rules above.
