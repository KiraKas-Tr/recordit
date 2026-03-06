# bd-8du2: XCTest Target for App Integration Paths

## Scope

`bd-8du2` ports high-signal app integration smoke coverage into executable XCTest form for app-target validation.

Delivered:

- New unit test target: `RecorditAppTests` in `Recordit.xcodeproj`
- New XCTest source: `app/RecorditAppTests/RecorditAppTests.swift`

## Test Coverage Added

`RecorditAppTests.swift` includes deterministic integration-path tests for:

1. preview app environment runtime + preflight contract flow
2. runtime binary readiness failure mapping (`invalid_override`)
3. runtime start/stop bounded finalization path in `RuntimeViewModel`
4. app-shell startup routing to recovery when runtime readiness is unavailable

These map directly to app-shell/view-model/service integration behavior and include explicit assertions with actionable failure diagnostics.

## Validation Commands

```bash
xcodebuild -project Recordit.xcodeproj -scheme RecorditAppTests -configuration Debug -destination 'platform=macOS,arch=arm64' -derivedDataPath .build/recordit-tests CODE_SIGNING_ALLOWED=NO test
```

## Validation Result

- `** TEST SUCCEEDED **`
- `Executed 4 tests, with 0 failures (0 unexpected)`

Target dependency graph during run confirms test target executes in app-target context:

- `Target 'RecorditAppTests' ... Explicit dependency on target 'RecorditApp'`

## Outcome

Acceptance criteria satisfied: a real XCTest target now validates high-signal AppShell/ViewModel/Service integration paths with deterministic fixtures and executable assertions.
