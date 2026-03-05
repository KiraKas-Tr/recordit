import Foundation

public struct PendingSessionActionResult: Equatable, Sendable {
    public var sessionID: String
    public var finalState: PendingTranscriptionState
    public var processIdentifier: Int32

    public init(sessionID: String, finalState: PendingTranscriptionState, processIdentifier: Int32) {
        self.sessionID = sessionID
        self.finalState = finalState
        self.processIdentifier = processIdentifier
    }
}

public protocol PendingSessionTranscribing: Sendable {
    func transcribePendingSession(
        summary: SessionSummaryDTO,
        timeoutSeconds: TimeInterval
    ) async throws -> PendingSessionActionResult
}

public struct PendingSessionTranscriptionService: PendingSessionTranscribing {
    public typealias DataWriter = @Sendable (Data, URL) throws -> Void
    public typealias DataReader = @Sendable (URL) throws -> Data
    public typealias Sleep = @Sendable (UInt64) async throws -> Void

    private let runtimeService: any RuntimeService
    private let pendingSidecarService: any PendingSessionSidecarService
    private let transitionService: PendingSessionTransitionService
    private let finalizerService: any PendingSessionFinalizing
    private let dataWriter: DataWriter
    private let dataReader: DataReader
    private let sleep: Sleep
    private let pollIntervalNanoseconds: UInt64

    public init(
        runtimeService: any RuntimeService,
        pendingSidecarService: any PendingSessionSidecarService = FileSystemPendingSessionSidecarService(),
        transitionService: PendingSessionTransitionService = PendingSessionTransitionService(),
        finalizerService: any PendingSessionFinalizing = PendingSessionFinalizerService(),
        dataWriter: @escaping DataWriter = { data, destination in try data.write(to: destination, options: .atomic) },
        dataReader: @escaping DataReader = {
            let handle = try FileHandle(forReadingFrom: $0)
            defer { try? handle.close() }
            return try handle.readToEnd() ?? Data()
        },
        sleep: @escaping Sleep = { nanoseconds in try await Task.sleep(nanoseconds: nanoseconds) },
        pollIntervalNanoseconds: UInt64 = 250_000_000
    ) {
        self.runtimeService = runtimeService
        self.pendingSidecarService = pendingSidecarService
        self.transitionService = transitionService
        self.finalizerService = finalizerService
        self.dataWriter = dataWriter
        self.dataReader = dataReader
        self.sleep = sleep
        self.pollIntervalNanoseconds = pollIntervalNanoseconds
    }

    public func transcribePendingSession(
        summary: SessionSummaryDTO,
        timeoutSeconds: TimeInterval = 120
    ) async throws -> PendingSessionActionResult {
        guard summary.mode == .recordOnly else {
            throw invalidInput(
                "Only record-only sessions can be transcribed via the pending-session action."
            )
        }
        guard summary.readyToTranscribe, summary.pendingTranscriptionState == .readyToTranscribe else {
            throw invalidInput("Session is not ready to transcribe.")
        }

        let sessionRoot = summary.rootPath.standardizedFileURL
        let pendingURL = sessionRoot.appendingPathComponent("session.pending.json")
        let sidecar = try pendingSidecarService.loadPendingSidecar(at: pendingURL)
        guard sidecar.transcriptionState == .readyToTranscribe else {
            throw invalidInput("Pending sidecar is not in ready_to_transcribe state.")
        }

        let transcribingState = try transitionService.transition(
            from: sidecar.transcriptionState,
            event: .transcriptionStarted
        )
        _ = try writeSidecar(
            current: sidecar,
            sessionRoot: sessionRoot,
            state: transcribingState
        )

        let launch: RuntimeLaunchResult
        do {
            launch = try await runtimeService.startSession(
                request: RuntimeStartRequest(
                    mode: .offline,
                    outputRoot: sessionRoot,
                    inputWav: URL(fileURLWithPath: sidecar.wavPath)
                )
            )
        } catch {
            let detail = String(describing: error)
            _ = try? markFailedAndPersistContext(
                current: sidecar,
                sessionRoot: sessionRoot,
                message: detail
            )
            throw error
        }

        do {
            let completed = try await waitForManifestResult(
                sessionRoot: sessionRoot,
                timeoutSeconds: timeoutSeconds
            )
            if completed {
                let completedState = try transitionService.transition(
                    from: transcribingState,
                    event: .transcriptionCompleted
                )
                _ = try writeSidecar(
                    current: sidecar,
                    sessionRoot: sessionRoot,
                    state: completedState
                )
                try finalizerService.finalizePendingSession(sessionRoot: sessionRoot)
                return PendingSessionActionResult(
                    sessionID: summary.sessionID,
                    finalState: completedState,
                    processIdentifier: launch.processIdentifier
                )
            }

            let failedState = try transitionService.transition(
                from: transcribingState,
                event: .transcriptionFailed
            )
            _ = try writeSidecar(current: sidecar, sessionRoot: sessionRoot, state: failedState)
            try writeRetryContext(
                sessionRoot: sessionRoot,
                message: "manifest reported failed session status"
            )
            throw AppServiceError(
                code: .processExitedUnexpectedly,
                userMessage: "Deferred transcription failed.",
                remediation: "Review diagnostics, then retry transcribing this pending session."
            )
        } catch let serviceError as AppServiceError {
            _ = try? markFailedAndPersistContext(
                current: sidecar,
                sessionRoot: sessionRoot,
                message: serviceError.debugDetail ?? serviceError.userMessage
            )
            throw serviceError
        } catch {
            _ = try? markFailedAndPersistContext(
                current: sidecar,
                sessionRoot: sessionRoot,
                message: String(describing: error)
            )
            throw AppServiceError(
                code: .unknown,
                userMessage: "Deferred transcription did not complete.",
                remediation: "Retry the action after checking runtime health.",
                debugDetail: String(describing: error)
            )
        }
    }

    private func markFailedAndPersistContext(
        current: PendingSessionSidecarDTO,
        sessionRoot: URL,
        message: String
    ) throws -> PendingSessionSidecarDTO {
        let failedState = try transitionService.transition(
            from: .transcribing,
            event: .transcriptionFailed
        )
        let updated = try writeSidecar(current: current, sessionRoot: sessionRoot, state: failedState)
        try writeRetryContext(sessionRoot: sessionRoot, message: message)
        return updated
    }

    private func writeSidecar(
        current: PendingSessionSidecarDTO,
        sessionRoot: URL,
        state: PendingTranscriptionState
    ) throws -> PendingSessionSidecarDTO {
        let createdAt = parseISO8601(current.createdAtUTC) ?? Date(timeIntervalSince1970: 0)
        let request = PendingSessionSidecarWriteRequest(
            sessionID: current.sessionID,
            sessionRoot: sessionRoot,
            wavPath: URL(fileURLWithPath: current.wavPath),
            createdAt: createdAt,
            mode: .recordOnly,
            transcriptionState: state
        )
        return try pendingSidecarService.writePendingSidecar(request)
    }

    private func waitForManifestResult(
        sessionRoot: URL,
        timeoutSeconds: TimeInterval
    ) async throws -> Bool {
        let manifestURL = sessionRoot.appendingPathComponent("session.manifest.json")
        let deadline = Date().addingTimeInterval(max(timeoutSeconds, 1))

        while Date() < deadline {
            if let status = try loadManifestStatus(manifestURL: manifestURL) {
                return status != .failed
            }
            try await sleep(pollIntervalNanoseconds)
        }

        throw AppServiceError(
            code: .timeout,
            userMessage: "Deferred transcription timed out.",
            remediation: "Retry after checking runtime binaries and model availability.",
            debugDetail: "timeout_seconds=\(timeoutSeconds)"
        )
    }

    private enum ManifestSessionStatus: String {
        case ok
        case degraded
        case failed
    }

    private func loadManifestStatus(manifestURL: URL) throws -> ManifestSessionStatus? {
        guard FileManager.default.fileExists(atPath: manifestURL.path) else {
            return nil
        }
        let data = try dataReader(manifestURL)
        guard let payload = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let summary = payload["session_summary"] as? [String: Any],
              let raw = (summary["session_status"] as? String)?.lowercased(),
              let status = ManifestSessionStatus(rawValue: raw) else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Deferred transcription output is malformed.",
                remediation: "Retry and regenerate the session manifest.",
                debugDetail: manifestURL.path
            )
        }
        return status
    }

    private func writeRetryContext(sessionRoot: URL, message: String) throws {
        struct RetryContext: Codable {
            var failedAtUTC: String
            var message: String

            enum CodingKeys: String, CodingKey {
                case failedAtUTC = "failed_at_utc"
                case message
            }
        }

        let context = RetryContext(
            failedAtUTC: ISO8601DateFormatter().string(from: Date()),
            message: message
        )
        let payload = try JSONEncoder().encode(context)
        let retryContextURL = sessionRoot.appendingPathComponent("session.pending.retry.json")
        try dataWriter(payload, retryContextURL)
    }

    private func parseISO8601(_ value: String) -> Date? {
        let formatter = ISO8601DateFormatter()
        return formatter.date(from: value)
    }

    private func invalidInput(_ message: String) -> AppServiceError {
        AppServiceError(
            code: .invalidInput,
            userMessage: message,
            remediation: "Refresh sessions, then run the action on a ready pending session."
        )
    }
}
