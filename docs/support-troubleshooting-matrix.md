# Support Troubleshooting Matrix (Recordit.app Lane)

Date: 2026-03-05
Scope: post-cutover support for default `Recordit.app` journey.

## Usage

1. Identify the closest failure mode row.
2. Run deterministic checks exactly as listed.
3. Apply user-safe remediation in order.
4. Escalate only when escalation trigger is met.

## Matrix

| Failure Mode | Deterministic Signal / Check | User-Safe Remediation | Escalation Trigger | Escalation Target |
|---|---|---|---|---|
| Permissions denied (Screen Recording / Microphone) | Onboarding/preflight indicates missing permission; `make probe CAPTURE_SECS=3` fails permission checks | 1) Open System Settings -> Privacy and grant Screen Recording + Microphone for the active app. 2) Relaunch `Recordit.app`. 3) Re-run onboarding preflight. | Permission remains blocked after settings grant + app relaunch + one retry | Support -> Platform owner |
| Model missing or unreadable | Model setup step fails; `make setup-whispercpp-model` fails or model path unreadable | 1) Run `make setup-whispercpp-model`. 2) Re-select model in onboarding/model setup. 3) Retry start. | Model still unresolved after setup command succeeds | Support -> Runtime/model owner |
| Runtime start fails from main UI | Runtime status enters failed state on Start; app-level evidence differs from expected path | 1) Run onboarding preflight again. 2) Start a new session with default live mode. 3) Collect latest session manifest path for support note. | Reproducible start failure for two consecutive attempts on same machine | Support -> Runtime owner |
| Stop/finalization fails or summary unavailable | Stop action ends in failed/recovery path; summary/manifest unavailable | 1) Use in-app recovery actions (`retry stop`, `safe finalize`, `open artifacts`). 2) Confirm `session.manifest.json` exists. 3) Retry stop/finalization once. | Manifest missing or failed after recovery retry | Support -> Reliability owner |
| Packaged launch/install issue (DMG path) | DMG mount missing `Recordit.app` or `Applications`; mounted app does not launch | 1) Rebuild DMG: `make create-recordit-dmg ...`. 2) Verify mount layout (`Recordit.app` + `Applications`). 3) Reinstall to `Applications` and relaunch. | Mount layout wrong or mounted launch fails after rebuild/reinstall | Support -> Packaging/release owner |
| Packaged smoke gate regression | Latest packaged smoke summary reports `gate_pass=false` or contract field false | 1) Capture latest `summary.csv` + `status.txt`. 2) Compare failing keys against baseline checklist. 3) Hold release promotion. | Any required packaged gate key remains false after one rerun | Support -> Release owner (no-go) |
| App-level XCTest/XCUITest evidence regression | `artifacts/ci/xctest_evidence/*/summary.csv` shows `required_failed>0` or responsiveness thresholds false | 1) Re-run CI evidence lane. 2) Inspect failing step logs/xcresult. 3) Do not claim shipped proof until required steps pass. | Required gate still failing after rerun | Support -> QA owner (block promotion) |

## Evidence References

- GUI-first quickstart: `docs/operator-quickstart.md`
- Release rehearsal report: `docs/bd-55np-release-rehearsal-report.md`
- Release checklist: `docs/bd-b2qv-release-checklist.md`
- XCTest/XCUITest evidence lane: `scripts/ci_recordit_xctest_evidence.sh`
- Packaged smoke gate guide: `docs/gate-packaged-live-smoke.md`

## Escalation Rule

If any escalation trigger is met, preserve evidence artifacts first, then escalate with:
- timestamp
- failing command/check
- artifact paths (`summary.csv`, `status.txt`, manifest/log path)
- exact remediation steps already attempted
