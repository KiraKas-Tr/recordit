import Foundation

public actor ProcessBackedRuntimeService: RuntimeService {
    private let processManager: RuntimeProcessManager
    private let pendingSidecarService: any PendingSessionSidecarService
    private let stopTimeoutSeconds: TimeInterval
    private let pendingSidecarStopTimeoutSeconds: TimeInterval

    public init(
        processManager: RuntimeProcessManager = RuntimeProcessManager(),
        pendingSidecarService: any PendingSessionSidecarService = FileSystemPendingSessionSidecarService(),
        stopTimeoutSeconds: TimeInterval = 15,
        pendingSidecarStopTimeoutSeconds: TimeInterval = 2
    ) {
        self.processManager = processManager
        self.pendingSidecarService = pendingSidecarService
        self.stopTimeoutSeconds = stopTimeoutSeconds
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
            let outcome = try await processManager.control(
                processIdentifier: processIdentifier,
                action: action,
                timeoutSeconds: stopTimeoutSeconds
            )
            switch outcome.classification {
            case .success:
                return RuntimeControlResult(accepted: true, detail: "Process finished cleanly.")
            case .nonZeroExit(let code):
                throw AppServiceError(
                    code: .processExitedUnexpectedly,
                    userMessage: "Runtime process ended with an error.",
                    remediation: "Open diagnostics and retry the session.",
                    debugDetail: "exit_code=\(code)"
                )
            case .crashed(let signal):
                throw AppServiceError(
                    code: .processExitedUnexpectedly,
                    userMessage: "Runtime process crashed.",
                    remediation: "Retry the session. If this repeats, run preflight diagnostics.",
                    debugDetail: "signal=\(signal)"
                )
            case .timedOut:
                throw AppServiceError(
                    code: .timeout,
                    userMessage: "Runtime did not stop in time.",
                    remediation: "Retry stop, then use Cancel if needed.",
                    debugDetail: "control_timeout_seconds=\(stopTimeoutSeconds)"
                )
            case .launchFailure(let detail):
                throw AppServiceError(
                    code: .processLaunchFailed,
                    userMessage: "Runtime control failed.",
                    remediation: "Retry the action.",
                    debugDetail: detail
                )
            }
        } catch let managerError as RuntimeProcessManagerError {
            throw Self.mapManagerError(managerError)
        }
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
