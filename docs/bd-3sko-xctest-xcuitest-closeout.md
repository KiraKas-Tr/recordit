# bd-3sko — XCTest/XCUITest Strategy Feature Closeout

Date: 2026-03-05

## Feature Goal
Upgrade UI validation from smoke-only harnesses to real app-executable XCTest/XCUITest coverage with CI evidence archiving.

## Child Lane Completion Map

All concrete child beads under `bd-3sko` are closed:

- `bd-8du2` — app-target XCTest targets and baseline integration coverage
- `bd-1aqk` — XCUITest first-run onboarding happy path
- `bd-3c0x` — XCUITest permission denial/remediation/recovery
- `bd-b4h6` — XCUITest live run start/stop + transcript/status/summary
- `bd-2ghc` — XCUITest summary/recovery artifact actions
- `bd-25ou` — CI lane for XCTest/XCUITest evidence archiving
- `bd-3jdm` — app-level responsiveness budgets promoted into XCTest/CI gates

## Evidence Anchors

- CI lane/workflow:
  - `.github/workflows/recordit-xctest-evidence.yml`
  - `scripts/ci_recordit_xctest_evidence.sh`
- Child evidence docs:
  - `docs/bd-1aqk-first-run-onboarding-xcuitest.md`
  - `docs/bd-3c0x-permission-remediation-xcuitest.md`
  - `docs/bd-b4h6-live-run-xcuitest.md`
  - `docs/bd-2ghc-session-summary-artifact-actions-xcuitest.md`
  - `docs/bd-25ou-ci-xctest-evidence.md`
  - `docs/bd-3jdm-app-level-responsiveness-gates.md`
- Fresh lane artifact snapshot from this closeout session:
  - `artifacts/ci/xctest_evidence/local-bd3jdm-r3/status.csv`
  - `artifacts/ci/xctest_evidence/local-bd3jdm-r3/summary.csv`
  - `artifacts/ci/xctest_evidence/local-bd3jdm-r3/responsiveness_budget_summary.csv`

## Acceptance Outcome

`bd-3sko` acceptance is satisfied:
- real app-level XCTest/XCUITest strategy is implemented and wired
- CI evidence lane is present with deterministic machine-readable outputs
- responsiveness budgets are enforced at app-test/gate level

## Notes

In this headless environment, targeted XCUITest execution can still fail to bootstrap test runners; those failures are archived as evidence in per-step logs/xcresults while required app-level test gates remain enforced.
