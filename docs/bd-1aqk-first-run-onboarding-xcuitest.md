# bd-1aqk — XCUITest: first-run onboarding happy path

## Summary
Implemented a real `RecorditAppUITests` UI-test target and a deterministic first-run onboarding happy-path test that drives:

1. app launch into onboarding
2. permissions checks
3. model setup validation + preflight
4. ready step completion
5. transition to main runtime (`start_live_transcribe` visible)

## Files
- `app/RecorditAppUITests/RecorditAppUITests.swift`
- `app/RecorditApp/RecorditApp.swift`
- `app/RecorditApp/OnboardingFlowView.swift`
- `Recordit.xcodeproj/project.pbxproj`
- `Recordit.xcodeproj/xcshareddata/xcschemes/RecorditApp.xcscheme`

## Determinism hooks used by UI test
- Launch arg/env gate in app entrypoint:
  - `--ui-test-mode` or `RECORDIT_UI_TEST_MODE=1` => use `.preview()` DI
  - `RECORDIT_FORCE_FIRST_RUN=1` => force onboarding root
- UI test sets runtime binary env overrides to executable stubs:
  - `RECORDIT_RUNTIME_BINARY=/usr/bin/true`
  - `SEQUOIA_CAPTURE_BINARY=/usr/bin/true`
- Stable onboarding accessibility IDs added for selectors:
  - step containers, nav controls, and key onboarding action buttons

## Validation

### 1. Project wiring
```bash
xcodebuild -list -project Recordit.xcodeproj
```
Result: target list includes `RecorditAppUITests`; scheme `RecorditApp` includes both `RecorditAppTests` and `RecorditAppUITests` testables.

### 2. Test build (includes UI test bundle)
```bash
xcodebuild build-for-testing \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data
```
Result: `** TEST BUILD SUCCEEDED **`

### 3. Regression check for existing XCTest target
```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppTests
```
Result: `** TEST SUCCEEDED **` (4/4 passing)

### 4. UI test execution attempt in this environment
Attempted:
```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppUITests/RecorditAppUITests/testFirstRunOnboardingHappyPathTransitionsToMainRuntime
```
Observed behavior: build/test runner launches, but UI-test execution does not complete in this headless agent environment (runner remains active without finishing), so the run was terminated.

## Follow-up
Run the new UI test from an interactive macOS login session (with UI automation permissions available):

```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppUITests/RecorditAppUITests/testFirstRunOnboardingHappyPathTransitionsToMainRuntime
```
