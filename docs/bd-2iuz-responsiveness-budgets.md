# bd-2iuz: Time-to-First-Transcript and Responsiveness Guardrails

## Goal

Define explicit latency budgets and deterministic pass/fail evaluation for user-visible responsiveness surfaces:
1. first stable transcript line
2. stop-to-summary transition
3. sessions search responsiveness

## Delivered

1. `app/Performance/ResponsivenessBudgetService.swift`
2. `app/Performance/responsiveness_budget_smoke.swift`

## What Landed

1. Added explicit metric vocabulary (`ResponsivenessMetric`):
- `first_stable_transcript_ms`
- `stop_to_summary_ms`
- `sessions_search_ms`

2. Added concrete default budgets (`ResponsivenessBudgetService.defaultBudgets`):
- first stable transcript: `<= 3500 ms`
- stop-to-summary: `<= 2000 ms`
- sessions search: `<= 250 ms`

3. Added deterministic measurement and report model:
- `ResponsivenessMeasurement`
- `ResponsivenessBudgetReport`
- `ResponsivenessViolation`
- violation reasons:
  - `missing_measurement`
  - `exceeded_budget`

4. Added regression gate mapping:
- `gateFailure(for:)` returns `AppServiceError(.timeout)` for failing reports
- debug detail includes failed metric IDs for CI/diagnostics consumption

5. Added deterministic violation ordering:
- missing measurements first
- then exceeded budgets by larger overage
- stable tie-break on metric id

## Acceptance Mapping

1. Budgets are explicitly defined for first stable transcript, stop-to-summary, and sessions search responsiveness.
2. Measurements are evaluated into deterministic pass/fail reports.
3. Regressions are flagged via failing report violations and gate error mapping.

## Validation

`responsiveness_budget_smoke.swift` covers:
1. pass path with all metrics under budget
2. failure path with both exceeded-budget and missing-measurement violations
3. deterministic violation ordering
4. gate failure mapping to `AppServiceError(.timeout)` with failed-metric diagnostics
