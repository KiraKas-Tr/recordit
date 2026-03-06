# bd-b4h6 — XCUITest live run start/stop with transcript updates

## Summary
Added a live-runtime UI test that validates start/stop user flow, in-run status/transcript updates, and completion surface expectations.

## Delivered
- `app/RecorditAppUITests/RecorditAppUITests.swift`
  - Added `testLiveRunStartStopShowsRuntimeStatusTranscriptAndSummary`.
  - Flow/Assertions:
    1. complete first-run onboarding happy path to main runtime
    2. assert initial runtime status is `Idle`
    3. tap `Start`, then assert runtime status transitions to `Running`
    4. assert transcript includes in-run event evidence (`Start requested for Live Transcribe`)
    5. tap `Stop`, then assert runtime status transitions to `Completed`
    6. assert transcript includes completion evidence (`Live stop completed successfully`)
    7. assert `Session Summary` surface appears

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

### 2) Existing XCTest regression check
```bash
xcodebuild test \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  -only-testing:RecorditAppTests
```
Result: `** TEST SUCCEEDED **` (4/4)

### 3) Targeted UI test execution attempt
```bash
xcodebuild test-without-building \
  -xctestrun .build/recordit-derived-data/Build/Products/RecorditApp_macosx15.5-arm64.xctestrun \
  -destination 'platform=macOS,arch=arm64' \
  -only-testing:RecorditAppUITests/RecorditAppUITests/testLiveRunStartStopShowsRuntimeStatusTranscriptAndSummary
```
Result in this headless agent environment: `TEST EXECUTE FAILED` before test start
- `RecorditAppUITests-Runner ... Early unexpected exit ... Test crashed with signal kill before starting test execution.`

## Notes
The XCUITest implementation for live-run start/stop transcript/status coverage is in place and compiles in the shared test target; execution failure observed here is environment-level UI-test runner bootstrap instability.
