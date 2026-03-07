import Foundation

public actor ProcessBackedRuntimeService: RuntimeService {
    private enum StopCompletionStage: String {
        case gracefulHandshake = "graceful_handshake"
        case interruptFallback = "interrupt_fallback"
        case terminateFallback = "terminate_fallback"
        case terminateTimeout = "terminate_timeout"
    }

    private struct StopTimeoutBudget {
        let totalSeconds: TimeInterval
        let gracefulSeconds: TimeInterval
        let interruptSeconds: TimeInterval
        let terminateSeconds: TimeInterval
    }

    private struct StopControlTelemetry {
        let totalTimeoutSeconds: TimeInterval
        let gracefulTimeoutSeconds: TimeInterval
        let interruptTimeoutSeconds: TimeInterval
        let terminateTimeoutSeconds: TimeInterval
        var gracefulRequestWritten: Bool
        var gracefulWaitMs: UInt64
        var interruptWaitMs: UInt64
        var terminateWaitMs: UInt64
        var stage: StopCompletionStage?
        var escalationReason: String?
    }

    private let processManager: RuntimeProcessManager
    private let pendingSidecarService: any PendingSessionSidecarService
    private let stopTimeoutSeconds: TimeInterval
    private let gracefulStopTimeoutSeconds: TimeInterval
    private let pendingSidecarStopTimeoutSeconds: TimeInterval

    public init(
        processManager: RuntimeProcessManager = RuntimeProcessManager(),
        pendingSidecarService: any PendingSessionSidecarService = FileSystemPendingSessionSidecarService(),
        stopTimeoutSeconds: TimeInterval = 15,
        gracefulStopTimeoutSeconds: TimeInterval = 2,
        pendingSidecarStopTimeoutSeconds: TimeInterval = 2
    ) {
        self.processManager = processManager
        self.pendingSidecarService = pendingSidecarService
        self.stopTimeoutSeconds = stopTimeoutSeconds
        self.gracefulStopTimeoutSeconds = gracefulStopTimeoutSeconds
        self.pendingSidecarStopTimeoutSeconds = pendingSidecarStopTimeoutSeconds
    }

    public func startSession(request: RuntimeStartRequest) async throws -> RuntimeLaunchResult {
        do {
            let launch = try await processManager.launch(request: request)
            if request.mode == .recordOnly {
                try await writePendingSidecarAfterLaunch(request: request, launch: launch)
            }
            return RuntimeLaunchResult(
                processIdentifier: launch.processIdentifier,
                sessionRoot: launch.sessionRoot,
                startedAt: launch.startedAt
            )
        } catch let managerError as RuntimeProcessManagerError {
            throw Self.mapManagerError(managerError)
        } catch {
            throw AppServiceError(
                code: .processLaunchFailed,
                userMessage: "Could not start the runtime process.",
                remediation: "Verify runtime binaries are installed and retry.",
                debugDetail: String(describing: error)
            )
        }
    }

    private func writePendingSidecarAfterLaunch(
        request: RuntimeStartRequest,
        launch: RuntimeProcessLaunch
    ) async throws {
        let initialState: PendingTranscriptionState = request.modelPath == nil ? .pendingModel : .readyToTranscribe
        do {
            let sidecarRequest = PendingSessionSidecarWriteRequest(
                sessionID: launch.sessionRoot.lastPathComponent,
                sessionRoot: launch.sessionRoot,
                wavPath: launch.sessionRoot.appendingPathComponent("session.wav"),
                createdAt: launch.startedAt,
                mode: .recordOnly,
                transcriptionState: initialState
            )
            _ = try pendingSidecarService.writePendingSidecar(sidecarRequest)
        } catch {
            _ = try? await processManager.control(
                processIdentifier: launch.processIdentifier,
                action: .cancel,
                timeoutSeconds: pendingSidecarStopTimeoutSeconds
            )
            if let serviceError = error as? AppServiceError {
                throw serviceError
            }
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not initialize pending session metadata.",
                remediation: "Retry recording. If this persists, verify session folder permissions.",
                debugDetail: String(describing: error)
            )
        }
    }

    public func controlSession(processIdentifier: Int32, action: RuntimeControlAction) async throws -> RuntimeControlResult {
        do {
            if let settledOutcome = await processManager.pollControlOutcome(
                processIdentifier: processIdentifier,
                action: action
            ) {
                return try mapControlOutcome(settledOutcome, requestedAction: action)
            }

            let outcome: RuntimeProcessControlOutcome
            if action == .stop {
                let budget = stopTimeoutBudget()
                var telemetry = StopControlTelemetry(
                    totalTimeoutSeconds: budget.totalSeconds,
                    gracefulTimeoutSeconds: budget.gracefulSeconds,
                    interruptTimeoutSeconds: budget.interruptSeconds,
                    terminateTimeoutSeconds: budget.terminateSeconds,
                    gracefulRequestWritten: false,
                    gracefulWaitMs: 0,
                    interruptWaitMs: 0,
                    terminateWaitMs: 0,
                    stage: nil,
                    escalationReason: nil
                )
                let gracefulStopRequestURL = await gracefulStopRequestURL(processIdentifier: processIdentifier)
                defer {
                    removeGracefulStopRequest(at: gracefulStopRequestURL)
                }
                telemetry.gracefulRequestWritten = (try? writeGracefulStopRequest(at: gracefulStopRequestURL)) ?? false

                let gracefulStartedAt = Date()
                if telemetry.gracefulRequestWritten,
                   let gracefulOutcome = try await waitForNaturalStopOutcome(
                    processIdentifier: processIdentifier,
                    requestedAction: action,
                    timeoutSeconds: budget.gracefulSeconds
                ) {
                    telemetry.gracefulWaitMs = elapsedMilliseconds(since: gracefulStartedAt)
                    telemetry.stage = .gracefulHandshake
                    outcome = gracefulOutcome
                } else {
                    telemetry.gracefulWaitMs = elapsedMilliseconds(since: gracefulStartedAt)
                    telemetry.escalationReason = telemetry.gracefulRequestWritten
                        ? "graceful_handshake_timeout"
                        : "graceful_request_unavailable"

                    let interruptStartedAt = Date()
                    let interruptOutcome = try await processManager.control(
                        processIdentifier: processIdentifier,
                        action: .stop,
                        timeoutSeconds: budget.interruptSeconds,
                        killOnTimeout: false
                    )
                    telemetry.interruptWaitMs = elapsedMilliseconds(since: interruptStartedAt)

                    if interruptOutcome.classification == .timedOut {
                        let terminateStartedAt = Date()
                        let terminateOutcome = try await processManager.control(
                            processIdentifier: processIdentifier,
                            action: .cancel,
                            timeoutSeconds: budget.terminateSeconds,
                            killOnTimeout: true
                        )
                        telemetry.terminateWaitMs = elapsedMilliseconds(since: terminateStartedAt)
                        telemetry.stage = terminateOutcome.classification == .timedOut
                            ? .terminateTimeout
                            : .terminateFallback
                        telemetry.escalationReason = "interrupt_timeout"
                        outcome = terminateOutcome
                    } else {
                        telemetry.stage = .interruptFallback
                        outcome = interruptOutcome
                    }
                }
                return try mapControlOutcome(
                    outcome,
                    requestedAction: action,
                    stopTelemetry: telemetry
                )
            } else {
                outcome = try await processManager.control(
                    processIdentifier: processIdentifier,
                    action: action,
                    timeoutSeconds: stopTimeoutSeconds
                )
            }
            return try mapControlOutcome(outcome, requestedAction: action)
        } catch let managerError as RuntimeProcessManagerError {
            throw Self.mapManagerError(managerError)
        }
    }

    private func waitForNaturalStopOutcome(
        processIdentifier: Int32,
        requestedAction: RuntimeControlAction,
        timeoutSeconds: TimeInterval
    ) async throws -> RuntimeProcessControlOutcome? {
        let boundedTimeout = max(0, timeoutSeconds)
        if boundedTimeout == 0 {
            return await processManager.pollControlOutcome(
                processIdentifier: processIdentifier,
                action: requestedAction
            )
        }
        let deadline = Date().addingTimeInterval(boundedTimeout)
        while Date() < deadline {
            if let outcome = await processManager.pollControlOutcome(
                processIdentifier: processIdentifier,
                action: requestedAction
            ) {
                return outcome
            }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        return await processManager.pollControlOutcome(
            processIdentifier: processIdentifier,
            action: requestedAction
        )
    }

    private func gracefulStopRequestURL(processIdentifier: Int32) async -> URL? {
        guard let sessionRoot = await processManager.sessionRoot(processIdentifier: processIdentifier) else {
            return nil
        }
        return sessionRoot
            .appendingPathComponent("session.stop.request", isDirectory: false)
            .standardizedFileURL
    }

    private func writeGracefulStopRequest(at requestURL: URL?) throws -> Bool {
        guard let requestURL else {
            return false
        }
        try FileManager.default.createDirectory(
            at: requestURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try Data("stop\n".utf8).write(to: requestURL, options: .atomic)
        return true
    }

    private func removeGracefulStopRequest(at requestURL: URL?) {
        guard let requestURL else {
            return
        }
        try? FileManager.default.removeItem(at: requestURL)
    }

    private func boundedGracefulStopTimeout() -> TimeInterval {
        let boundedTotal = max(0.1, stopTimeoutSeconds)
        let requestedGrace = max(0.05, gracefulStopTimeoutSeconds)
        return min(requestedGrace, boundedTotal * 0.5)
    }

    private func stopTimeoutBudget() -> StopTimeoutBudget {
        let total = max(0.1, stopTimeoutSeconds)
        let graceful = boundedGracefulStopTimeout()
        let fallback = max(0, total - graceful)
        let interrupt = fallback * 0.5
        let terminate = max(0, fallback - interrupt)
        return StopTimeoutBudget(
            totalSeconds: total,
            gracefulSeconds: graceful,
            interruptSeconds: interrupt,
            terminateSeconds: terminate
        )
    }

    private func elapsedMilliseconds(since startedAt: Date) -> UInt64 {
        let elapsed = max(0, Date().timeIntervalSince(startedAt))
        return UInt64((elapsed * 1000).rounded())
    }

    private func stopTelemetryDetail(_ telemetry: StopControlTelemetry) -> String {
        let strategy = telemetry.stage?.rawValue ?? "unknown"
        let reason = telemetry.escalationReason ?? "none"
        return [
            "stop_strategy=\(strategy)",
            "graceful_request_written=\(telemetry.gracefulRequestWritten)",
            "graceful_wait_ms=\(telemetry.gracefulWaitMs)",
            "interrupt_wait_ms=\(telemetry.interruptWaitMs)",
            "terminate_wait_ms=\(telemetry.terminateWaitMs)",
            "stop_timeout_seconds=\(formatSeconds(telemetry.totalTimeoutSeconds))",
            "graceful_timeout_seconds=\(formatSeconds(telemetry.gracefulTimeoutSeconds))",
            "interrupt_timeout_seconds=\(formatSeconds(telemetry.interruptTimeoutSeconds))",
            "terminate_timeout_seconds=\(formatSeconds(telemetry.terminateTimeoutSeconds))",
            "escalation_reason=\(reason)",
        ].joined(separator: ", ")
    }

    private func formatSeconds(_ value: TimeInterval) -> String {
        String(format: "%.3f", value)
    }

    private func combineDetails(_ parts: String?...) -> String {
        parts
            .compactMap { $0?.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .joined(separator: ", ")
    }

    private func mapControlOutcome(
        _ outcome: RuntimeProcessControlOutcome,
        requestedAction: RuntimeControlAction,
        stopTelemetry: StopControlTelemetry? = nil
    ) throws -> RuntimeControlResult {
        let stopDetail = stopTelemetry.map(stopTelemetryDetail)

        switch outcome.classification {
        case .success:
            let detail = combineDetails(
                "Process finished cleanly.",
                stopDetail
            )
            return RuntimeControlResult(accepted: true, detail: detail)
        case .nonZeroExit(let code):
            let stderrDetail = Self.runtimeStderrDetail(sessionRoot: outcome.sessionRoot)
            let debugDetail = combineDetails(
                "exit_code=\(code)",
                stopDetail,
                stderrDetail
            )
            throw AppServiceError(
                code: .processExitedUnexpectedly,
                userMessage: "Runtime process ended with an error.",
                remediation: "Open diagnostics and retry the session.",
                debugDetail: debugDetail
            )
        case .crashed(let signal):
            throw AppServiceError(
                code: .processExitedUnexpectedly,
                userMessage: "Runtime process crashed.",
                remediation: "Retry the session. If this repeats, run preflight diagnostics.",
                debugDetail: combineDetails(
                    "signal=\(signal)",
                    stopDetail
                )
            )
        case .timedOut:
            let detail: String
            if requestedAction == .stop {
                detail = combineDetails(
                    stopDetail,
                    "control_timeout_seconds=\(formatSeconds(stopTimeoutBudget().totalSeconds))"
                )
            } else {
                detail = "control_timeout_seconds=\(formatSeconds(stopTimeoutSeconds))"
            }
            throw AppServiceError(
                code: .timeout,
                userMessage: "Runtime did not stop in time.",
                remediation: "Retry stop, then use Cancel if needed.",
                debugDetail: detail
            )
        case .launchFailure(let detail):
            throw AppServiceError(
                code: .processLaunchFailed,
                userMessage: "Runtime control failed.",
                remediation: "Retry the action.",
                debugDetail: detail
            )
        }
    }

    private static func runtimeStderrDetail(sessionRoot: URL?) -> String? {
        guard let sessionRoot else {
            return nil
        }
        let stderrPath = sessionRoot
            .appendingPathComponent("runtime.stderr.log", isDirectory: false)
            .standardizedFileURL
        guard FileManager.default.fileExists(atPath: stderrPath.path) else {
            return "stderr_log_missing=\(stderrPath.path)"
        }

        guard let data = FileManager.default.contents(atPath: stderrPath.path), !data.isEmpty else {
            return "stderr_log=\(stderrPath.path) (empty)"
        }

        let maxBytes = 4096
        let tailData: Data
        if data.count > maxBytes {
            tailData = data.suffix(maxBytes)
        } else {
            tailData = data
        }
        let rawTail = String(decoding: tailData, as: UTF8.self)
        let normalizedTail = rawTail
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .split(separator: "\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .joined(separator: " | ")

        if normalizedTail.isEmpty {
            return "stderr_log=\(stderrPath.path) (non-empty, non-text)"
        }
        return "stderr_log=\(stderrPath.path), stderr_tail=\(normalizedTail)"
    }

    private static func mapManagerError(_ error: RuntimeProcessManagerError) -> AppServiceError {
        switch error {
        case let .invalidPath(field, detail):
            return AppServiceError(
                code: .invalidInput,
                userMessage: "Runtime configuration path is invalid.",
                remediation: "Use absolute paths for runtime inputs/outputs.",
                debugDetail: "field=\(field), detail=\(detail)"
            )
        case let .missingRequiredValue(field):
            return AppServiceError(
                code: .invalidInput,
                userMessage: "A required runtime input is missing.",
                remediation: "Fill all required fields and retry.",
                debugDetail: "field=\(field)"
            )
        case let .binaryNotFound(name):
            return AppServiceError(
                code: .runtimeUnavailable,
                userMessage: "Required runtime binary is missing.",
                remediation: "Install Recordit runtime binaries or set explicit binary paths.",
                debugDetail: "binary=\(name)"
            )
        case let .binaryNotExecutable(path):
            return AppServiceError(
                code: .runtimeUnavailable,
                userMessage: "Runtime binary is not executable.",
                remediation: "Fix binary permissions and retry.",
                debugDetail: path
            )
        case let .launchFailed(detail):
            return AppServiceError(
                code: .processLaunchFailed,
                userMessage: "Could not launch runtime process.",
                remediation: "Retry after checking runtime installation and permissions.",
                debugDetail: detail
            )
        case let .unknownProcess(processIdentifier):
            return AppServiceError(
                code: .runtimeUnavailable,
                userMessage: "Session process is no longer available.",
                remediation: "Start a new session.",
                debugDetail: "pid=\(processIdentifier)"
            )
        }
    }
}
