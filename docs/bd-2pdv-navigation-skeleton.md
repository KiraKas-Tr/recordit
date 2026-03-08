# bd-2pdv Navigation Skeleton

Last updated: 2026-03-05

## Scope

This bead adds a coordinator-owned route state machine for the SwiftUI app shell with these primary flows:

1. onboarding
2. main runtime
3. sessions
4. recovery
5. summary/error overlays

Implementation files:
- `app/Navigation/NavigationModels.swift`
- `app/Navigation/AppNavigationCoordinator.swift`
- `app/AppShell/AppShellViewModel.swift`

## Single Source of Truth

`AppNavigationCoordinator.state` is the canonical route state.  
Views must consume route state through `AppShellViewModel.navigationState`; they must not mutate routes directly.

## Route Model

Top-level route:
- `AppRootRoute.onboarding`
- `AppRootRoute.mainRuntime`
- `AppRootRoute.sessions`
- `AppRootRoute.recovery`

Nested/sub-routes:
- sessions path: `.list -> .detail(sessionID)`
- recovery subtype: permission/model/runtime recovery lane
- runtime overlays: session summary + runtime error

## Deep-Link and Back Rules

Deep-link behavior:
1. `sessionsList` always lands at `sessions + [.list]`.
2. `sessionDetail(id)` always lands at `sessions + [.list, .detail(id)]`.
3. `recovery(errorCode)` always lands at `recovery` with mapped subtype.

Back-navigation behavior:
1. If an overlay is visible, `back` dismisses overlay first.
2. In sessions detail, `back` pops to sessions list.
3. On sessions list, `back` returns to main runtime.
4. On recovery, `back` returns to main runtime.
5. On onboarding/main runtime without nested route, `back` is a no-op.

## No Dead-End Guarantees

1. Every recovery route has a deterministic back target (`mainRuntime`).
2. Every sessions detail route has a deterministic back target (`sessionsList`).
3. Overlay routes always have a deterministic dismiss/back path.

## Verification Commands

Typecheck app-shell scaffold:

```bash
swiftc -parse-as-library \
  app/Services/ServiceInterfaces.swift \
  app/Services/MockServices.swift \
  app/ViewModels/RuntimeViewModel.swift \
  app/Navigation/NavigationModels.swift \
  app/Navigation/AppNavigationCoordinator.swift \
  app/AppShell/AppShellViewModel.swift
```

Deep-link/back smoke assertions:

```bash
swift app/Navigation/navigation_smoke.swift
```
