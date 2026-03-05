import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("responsiveness_budget_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func runSmoke() {
    let fixedNow = Date(timeIntervalSince1970: 1_736_000_000)
    let service = ResponsivenessBudgetService(nowProvider: { fixedNow })

    let passingReport = service.evaluate(
        measurements: [
            ResponsivenessMeasurement(metric: .firstStableTranscriptMs, observedMilliseconds: 2_900),
            ResponsivenessMeasurement(metric: .stopToSummaryMs, observedMilliseconds: 1_700),
            ResponsivenessMeasurement(metric: .sessionsSearchMs, observedMilliseconds: 120),
        ]
    )

    check(passingReport.measuredAt == fixedNow, "report should use deterministic timestamp provider")
    check(passingReport.isPassing, "passing measurements should satisfy budgets")
    check(passingReport.violations.isEmpty, "passing measurements should have no violations")
    check(service.gateFailure(for: passingReport) == nil, "gateFailure should be nil when report passes")

    let failingReport = service.evaluate(
        measurements: [
            ResponsivenessMeasurement(metric: .firstStableTranscriptMs, observedMilliseconds: 4_300),
            ResponsivenessMeasurement(metric: .sessionsSearchMs, observedMilliseconds: 120),
        ]
    )

    check(!failingReport.isPassing, "missing/exceeded metrics should fail report")
    check(failingReport.violations.count == 2, "expected one exceeded and one missing violation")

    let firstViolation = failingReport.violations[0]
    check(
        firstViolation.metric == .stopToSummaryMs && firstViolation.reason == .missingMeasurement,
        "missing measurement should be prioritized in violation ordering"
    )

    let secondViolation = failingReport.violations[1]
    check(
        secondViolation.metric == .firstStableTranscriptMs && secondViolation.reason == .exceededBudget,
        "exceeded budget violation should be captured"
    )
    check(secondViolation.observedMilliseconds == 4_300, "exceeded violation should include observed latency")

    let failure = service.gateFailure(for: failingReport)
    check(failure?.code == .timeout, "failed report should map to timeout gate failure")
    check(
        failure?.debugDetail?.contains("first_stable_transcript_ms") == true,
        "gate failure should include failed metric identifiers"
    )
}

@main
struct ResponsivenessBudgetSmokeMain {
    static func main() {
        runSmoke()
        print("responsiveness_budget_smoke: PASS")
    }
}
