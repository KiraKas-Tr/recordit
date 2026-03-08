# bd-3jdm — App-Level Responsiveness Budget Gates

Date: 2026-03-05

## Scope
Promote responsiveness budgets into app-level XCTest/CI gates so first-transcript and stop-to-summary latency constraints are asserted beyond service-level smoke coverage.

## Implementation

1. App-level responsiveness XCTest gate
- File: `app/RecorditAppTests/RecorditAppTests.swift`
- Added `testAppLevelResponsivenessBudgetsForLiveRun()`:
  - boots a real `MainSessionController` over `AppEnvironment.preview().replacing(...)`
  - measures elapsed time from `startSession()` to running transcript signal
  - measures elapsed time from `stopSession()` to summary availability (`latestFinalizationSummary`)
  - evaluates measurements with `ResponsivenessBudgetService` budgets:
    - `first_stable_transcript_ms <= 3500`
    - `stop_to_summary_ms <= 2000`

2. CI gate wiring
- File: `scripts/ci_recordit_xctest_evidence.sh`
- Added required step `responsiveness_budget_gate` (xcodebuild targeted test execution).
- Added deterministic threshold rows to lane summary output:
  - `threshold_first_stable_transcript_budget_ok`
  - `threshold_stop_to_summary_budget_ok`
  - `responsiveness_gate_pass`
- Added canonical responsiveness artifact emission at:
  - `artifacts/ci/xctest_evidence/<stamp>/responsiveness_budget_summary.csv`

3. Docs update
- File: `README.md`
- Updated XCTest/XCUITest lane section with responsiveness artifact + threshold fields.

## Validation

1. Full CI evidence lane (required steps pass)
```bash
XCTEST_EVIDENCE_STAMP=local-bd3jdm-r3 scripts/ci_recordit_xctest_evidence.sh
```
Result: script exit `0`, `overall_status=pass`, `required_failed=0`.

2. Dedicated app-level responsiveness test
```bash
xcodebuild test -project Recordit.xcodeproj -scheme RecorditApp -destination 'platform=macOS' -derivedDataPath .build/recordit-derived-data -only-testing:RecorditAppTests/RecorditAppTests/testAppLevelResponsivenessBudgetsForLiveRun
```
Result: `TEST SUCCEEDED` (1/1).

3. UBS (scoped)
```bash
UBS_MAX_DIR_SIZE_MB=9000 ubs app/RecorditAppTests/RecorditAppTests.swift scripts/ci_recordit_xctest_evidence.sh README.md
```
Result: exit `0` (`Critical: 0`, `Warning: 0`).

## Evidence Artifacts

- `artifacts/ci/xctest_evidence/local-bd3jdm-r3/status.csv`
- `artifacts/ci/xctest_evidence/local-bd3jdm-r3/summary.csv`
- `artifacts/ci/xctest_evidence/local-bd3jdm-r3/responsiveness_budget_summary.csv`

Key summary rows:
- `threshold_first_stable_transcript_budget_ok,true`
- `threshold_stop_to_summary_budget_ok,true`
- `responsiveness_gate_pass,true`
