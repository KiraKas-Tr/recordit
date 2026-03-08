# bd-3c0x — XCUITest permission denial -> remediation -> recovery

## Summary
Added deterministic UI-test coverage for permission denial/remediation/recovery and the launch-time preflight fixture hook needed to make the flow reproducible.

## Delivered
- `app/RecorditAppUITests/RecorditAppUITests.swift`
  - Added `testPermissionDenialRemediationRecoversToOnboardingProgression`.
  - Test asserts:
    1. permission denial state surfaces (`screen_recording` and `microphone` missing)
    2. remediation affordances are visible (`Open Screen Recording Settings`, `Open Microphone Settings`)
    3. screen-recording restart advisory appears/dismisses
    4. `Re-check` transitions to granted states and re-enables onboarding progression
    5. flow can proceed to model setup step
- `app/RecorditApp/RecorditApp.swift`
  - Added UI-test preflight scenario hook via env var:
    - `RECORDIT_UI_TEST_PREFLIGHT_SCENARIO=permission_recovery`
  - Implemented scripted preflight command runner returning deterministic sequence:
    1. first permission check -> denied payload
    2. subsequent checks -> granted payload
- `app/RecorditApp/OnboardingFlowView.swift`
  - Added selectors used by the new test:
    - permission row IDs (`permission_row_*`)
    - settings/remediation button IDs
    - screen restart advisory + dismiss IDs

## Validation

### 1) Compile UI test + app wiring
```bash
xcodebuild build-for-testing \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data
```
Result: `** TEST BUILD SUCCEEDED **`

### 2) Regression check on existing XCTest suite
```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppTests
```
Result: `** TEST SUCCEEDED **` (4/4)

### 3) Targeted UI-test execution attempt
```bash
xcodebuild test-without-building \
  -xctestrun .build/recordit-derived-data/Build/Products/RecorditApp_macosx15.5-arm64.xctestrun \
  -destination 'platform=macOS,arch=arm64' \
  -only-testing:RecorditAppUITests/RecorditAppUITests/testPermissionDenialRemediationRecoversToOnboardingProgression
```
Result in this headless agent environment: `TEST EXECUTE FAILED` before test start
- `RecorditAppUITests-Runner ... Early unexpected exit ... Test crashed with signal kill before starting test execution.`

## Notes
The denial/recovery XCUITest implementation and deterministic hooks are in place; execution failure is environment-level runner bootstrap instability (not compile-time/test-definition failure).
