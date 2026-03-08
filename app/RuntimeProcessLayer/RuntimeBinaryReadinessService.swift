import Foundation

public protocol RuntimeBinaryReadinessChecking: Sendable {
    func evaluateStartupReadiness() -> RuntimeBinaryReadinessReport
    func startupBlockingError(from report: RuntimeBinaryReadinessReport) -> AppServiceError?
}

public struct RuntimeBinaryReadinessService: RuntimeBinaryReadinessChecking {
    private let resolver: RuntimeBinaryResolver

    public init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        resolver = RuntimeBinaryResolver(environment: environment)
    }

    public init(resolver: RuntimeBinaryResolver) {
        self.resolver = resolver
    }

    public func evaluateStartupReadiness() -> RuntimeBinaryReadinessReport {
        resolver.startupReadinessReport()
    }

    public func startupBlockingError(from report: RuntimeBinaryReadinessReport) -> AppServiceError? {
        guard let blocking = report.firstBlockingCheck else {
            return nil
        }

        var detailParts = ["binary=\(blocking.binaryName)", "status=\(blocking.status.rawValue)"]
        if let resolvedPath = blocking.resolvedPath {
            detailParts.append("path=\(resolvedPath)")
        }
        if let debugDetail = blocking.debugDetail, !debugDetail.isEmpty {
            detailParts.append(debugDetail)
        }

        return AppServiceError(
            code: .runtimeUnavailable,
            userMessage: blocking.userMessage,
            remediation: blocking.remediation,
            debugDetail: detailParts.joined(separator: ", ")
        )
    }
}
