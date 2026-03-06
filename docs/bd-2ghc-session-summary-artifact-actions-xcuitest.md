# bd-2ghc — XCUITest session summary actions and recovery affordances

## Summary
Implemented XCUITest coverage for summary-sheet action affordances on successful completion and recovery-sheet affordances under deterministic runtime failure.

## Delivered
- `app/RecorditAppUITests/RecorditAppUITests.swift`
  - Enhanced `testLiveRunStartStopShowsRuntimeStatusTranscriptAndSummary` to assert summary action controls are present:
    - `Open Session Detail`
    - `Start New Session`
    - `Dismiss`
  - Added `testRuntimeStopFailureShowsRecoveryAffordances`:
    - drives onboarding -> live start -> stop under fixture-induced failure
    - asserts `Runtime Recovery` sheet and action affordances:
      - `resume_interrupted_session`
      - `safe_finalize_session`
      - `retry_stop_action`
      - `open_session_artifacts`
      - `start_new_session_action`
- `app/RecorditApp/RecorditApp.swift`
  - Added UI-test runtime scenario hook via `RECORDIT_UI_TEST_RUNTIME_SCENARIO=stop_failure`
  - Added `ScriptedUITestRuntimeService` fixture to deterministically fail stop control requests for recovery-surface testing
- `docs/bd-2ghc-session-summary-artifact-actions-xcuitest.md`

## Validation

### 1) Build-for-testing
```bash
xcodebuild build-for-testing \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data
```
Result: `** TEST BUILD SUCCEEDED **`

### 2) Existing XCTest regression
```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppTests
```
Result: `** TEST SUCCEEDED **` (4/4)

### 3) Targeted UI-test execute attempt for recovery case
```bash
xcodebuild test-without-building \
  -xctestrun .build/recordit-derived-data/Build/Products/RecorditApp_macosx15.5-arm64.xctestrun \
  -destination 'platform=macOS,arch=arm64' \
  -only-testing:RecorditAppUITests/RecorditAppUITests/testRuntimeStopFailureShowsRecoveryAffordances
```
Result in this headless agent environment: `TEST EXECUTE FAILED` before test start
- `RecorditAppUITests-Runner ... Early unexpected exit ... Test crashed with signal kill before starting test execution.`

## Notes
Coverage is implemented in the XCUITest suite with deterministic app-side fixtures for both success summary and failure recovery affordance validation. Execution failure remains an environment-level runner bootstrap issue.
