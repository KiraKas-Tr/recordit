import Foundation

public enum ResponsivenessMetric: String, CaseIterable, Codable, Sendable {
    case firstStableTranscriptMs = "first_stable_transcript_ms"
    case stopToSummaryMs = "stop_to_summary_ms"
    case sessionsSearchMs = "sessions_search_ms"

    public var displayName: String {
        switch self {
        case .firstStableTranscriptMs:
            return "First stable transcript"
        case .stopToSummaryMs:
            return "Stop-to-summary transition"
        case .sessionsSearchMs:
            return "Sessions search responsiveness"
        }
    }
}

public struct ResponsivenessBudget: Equatable, Sendable {
    public var metric: ResponsivenessMetric
    public var maxMilliseconds: UInt64

    public init(metric: ResponsivenessMetric, maxMilliseconds: UInt64) {
        self.metric = metric
        self.maxMilliseconds = maxMilliseconds
    }
}

public struct ResponsivenessMeasurement: Equatable, Sendable {
    public var metric: ResponsivenessMetric
    public var observedMilliseconds: UInt64

    public init(metric: ResponsivenessMetric, observedMilliseconds: UInt64) {
        self.metric = metric
        self.observedMilliseconds = observedMilliseconds
    }
}

public enum ResponsivenessViolationReason: String, Codable, Sendable {
    case exceededBudget = "exceeded_budget"
    case missingMeasurement = "missing_measurement"
}

public struct ResponsivenessViolation: Equatable, Sendable {
    public var metric: ResponsivenessMetric
    public var budgetMilliseconds: UInt64
    public var observedMilliseconds: UInt64?
    public var reason: ResponsivenessViolationReason

    public init(
        metric: ResponsivenessMetric,
        budgetMilliseconds: UInt64,
        observedMilliseconds: UInt64?,
        reason: ResponsivenessViolationReason
    ) {
        self.metric = metric
        self.budgetMilliseconds = budgetMilliseconds
        self.observedMilliseconds = observedMilliseconds
        self.reason = reason
    }
}

public struct ResponsivenessBudgetReport: Equatable, Sendable {
    public var measuredAt: Date
    public var measurements: [ResponsivenessMeasurement]
    public var violations: [ResponsivenessViolation]

    public init(
        measuredAt: Date,
        measurements: [ResponsivenessMeasurement],
        violations: [ResponsivenessViolation]
    ) {
        self.measuredAt = measuredAt
        self.measurements = measurements
        self.violations = violations
    }

    public var isPassing: Bool {
        violations.isEmpty
    }
}

public struct ResponsivenessBudgetService {
    public static let defaultBudgets: [ResponsivenessBudget] = [
        ResponsivenessBudget(metric: .firstStableTranscriptMs, maxMilliseconds: 3_500),
        ResponsivenessBudget(metric: .stopToSummaryMs, maxMilliseconds: 2_000),
        ResponsivenessBudget(metric: .sessionsSearchMs, maxMilliseconds: 250),
    ]

    private let budgets: [ResponsivenessBudget]
    private let nowProvider: () -> Date

    public init(
        budgets: [ResponsivenessBudget] = Self.defaultBudgets,
        nowProvider: @escaping () -> Date = Date.init
    ) {
        let byMetric = Dictionary(uniqueKeysWithValues: budgets.map { ($0.metric, $0) })
        self.budgets = ResponsivenessMetric.allCases.compactMap { byMetric[$0] }
        self.nowProvider = nowProvider
    }

    public func evaluate(measurements: [ResponsivenessMeasurement]) -> ResponsivenessBudgetReport {
        let latestByMetric = Dictionary(uniqueKeysWithValues: measurements.map { ($0.metric, $0) })
        let normalizedMeasurements = ResponsivenessMetric.allCases.compactMap { latestByMetric[$0] }

        var violations: [ResponsivenessViolation] = []
        violations.reserveCapacity(budgets.count)

        for budget in budgets {
            guard let measured = latestByMetric[budget.metric] else {
                violations.append(
                    ResponsivenessViolation(
                        metric: budget.metric,
                        budgetMilliseconds: budget.maxMilliseconds,
                        observedMilliseconds: nil,
                        reason: .missingMeasurement
                    )
                )
                continue
            }

            if measured.observedMilliseconds > budget.maxMilliseconds {
                violations.append(
                    ResponsivenessViolation(
                        metric: budget.metric,
                        budgetMilliseconds: budget.maxMilliseconds,
                        observedMilliseconds: measured.observedMilliseconds,
                        reason: .exceededBudget
                    )
                )
            }
        }

        let orderedViolations = violations.sorted(by: Self.violationOrder)
        return ResponsivenessBudgetReport(
            measuredAt: nowProvider(),
            measurements: normalizedMeasurements,
            violations: orderedViolations
        )
    }

    public func gateFailure(for report: ResponsivenessBudgetReport) -> AppServiceError? {
        guard !report.isPassing else {
            return nil
        }

        let failedMetrics = report.violations.map { $0.metric.rawValue }.joined(separator: ",")
        return AppServiceError(
            code: .timeout,
            userMessage: "Responsiveness budgets regressed.",
            remediation: "Capture diagnostics and resolve latency regressions before promoting this build.",
            debugDetail: "failed_metrics=\(failedMetrics)"
        )
    }

    private static func violationOrder(lhs: ResponsivenessViolation, rhs: ResponsivenessViolation) -> Bool {
        if lhs.reason != rhs.reason {
            return lhs.reason == .missingMeasurement
        }

        let lhsDelta = Int64((lhs.observedMilliseconds ?? 0)) - Int64(lhs.budgetMilliseconds)
        let rhsDelta = Int64((rhs.observedMilliseconds ?? 0)) - Int64(rhs.budgetMilliseconds)
        if lhsDelta != rhsDelta {
            return lhsDelta > rhsDelta
        }
        return lhs.metric.rawValue < rhs.metric.rawValue
    }
}
