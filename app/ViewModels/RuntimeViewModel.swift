import Foundation

@MainActor
public final class RuntimeViewModel {
    public enum RunState: Equatable {
        case idle
        case preparing
        case running(processID: Int32)
        case stopping(processID: Int32)
        case finalizing
        case completed
        case failed(AppServiceError)
    }

    public private(set) var state: RunState = .idle
    public private(set) var lastRejectedActionError: AppServiceError?

    public static let accessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "start_live_transcribe",
            label: "Start live transcription",
            hint: "Starts a live session after model validation succeeds."
        ),
        AccessibilityElementDescriptor(
            id: "stop_live_transcribe",
            label: "Stop live transcription",
            hint: "Stops the active session and finalizes artifacts."
        ),
        AccessibilityElementDescriptor(
            id: "runtime_status",
            label: "Runtime status",
            hint: "Announces launch, running, and failure state changes."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: ["start_live_transcribe", "stop_live_transcribe", "runtime_status"]
    )

    public static let keyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "start_live_shortcut",
            key: "return",
            modifiers: ["command"],
            actionSummary: "Start live transcription."
        ),
        KeyboardShortcutDescriptor(
            id: "stop_live_shortcut",
            key: ".",
            modifiers: ["command"],
            actionSummary: "Stop the active live session."
        ),
    ]

    private let runtimeService: RuntimeService
    private let manifestService: ManifestService
    private let modelService: ModelResolutionService
    private let finalStatusMapper: ManifestFinalStatusMapper

    public init(
        runtimeService: RuntimeService,
        manifestService: ManifestService,
        modelService: ModelResolutionService,
        finalStatusMapper: ManifestFinalStatusMapper = ManifestFinalStatusMapper()
    ) {
        self.runtimeService = runtimeService
        self.manifestService = manifestService
        self.modelService = modelService
        self.finalStatusMapper = finalStatusMapper
    }

    public func startLive(outputRoot: URL, explicitModelPath: URL?) async {
        guard transition(
            to: .preparing,
            allowedFrom: [.idle, .completed, .failed],
            action: "startLive",
            invalidUserMessage: "Session start is unavailable while another session transition is active.",
            invalidRemediation: "Wait for the current transition to finish, then try Start again."
        ) else {
            return
        }
        do {
            _ = try modelService.resolveModel(ModelResolutionRequest(explicitModelPath: explicitModelPath, backend: "whispercpp"))
            let result = try await runtimeService.startSession(
                request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot, modelPath: explicitModelPath)
            )
            _ = transition(
                to: .running(processID: result.processIdentifier),
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly before launch completed.",
                invalidRemediation: "Reset the run state and try starting again."
            )
        } catch let serviceError as AppServiceError {
            _ = transition(
                to: .failed(serviceError),
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly while handling launch failure.",
                invalidRemediation: "Reset the run state and retry launch."
            )
        } catch {
            _ = transition(
                to: .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not start session.",
                    remediation: "Try again. If this keeps happening, run preflight diagnostics first.",
                    debugDetail: String(describing: error)
                )
            )
            ,
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly while handling launch failure.",
                invalidRemediation: "Reset the run state and retry launch."
            )
        }
    }

    public func stopCurrentRun() async {
        guard case let .running(processID) = state else {
            rejectAction(
                action: "stopCurrentRun",
                userMessage: "Stop is only available while a session is running.",
                remediation: "Start a session first, then use Stop after runtime begins."
            )
            return
        }
        guard transition(
            to: .stopping(processID: processID),
            allowedFrom: [.running],
            action: "stopCurrentRun",
            invalidUserMessage: "Stop is unavailable because runtime state is no longer running.",
            invalidRemediation: "Refresh runtime state, then retry Stop."
        ) else {
            return
        }
        do {
            _ = try await runtimeService.controlSession(processIdentifier: processID, action: .stop)
            _ = transition(
                to: .finalizing,
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly during stop finalization.",
                invalidRemediation: "Load final status to recover state."
            )
        } catch let serviceError as AppServiceError {
            _ = transition(
                to: .failed(serviceError),
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly while handling stop failure.",
                invalidRemediation: "Refresh runtime state and retry control action."
            )
        } catch {
            _ = transition(
                to: .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not stop session cleanly.",
                    remediation: "Wait a few seconds and try Stop again.",
                    debugDetail: String(describing: error)
                )
            )
            ,
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly while handling stop failure.",
                invalidRemediation: "Refresh runtime state and retry control action."
            )
        }
    }

    public func loadFinalStatus(manifestPath: URL) {
        guard transition(
            to: .finalizing,
            allowedFrom: [.idle, .running, .stopping, .finalizing, .completed, .failed],
            action: "loadFinalStatus",
            invalidUserMessage: "Final status cannot be loaded while runtime launch is still preparing.",
            invalidRemediation: "Wait for launch to complete before loading final status."
        ) else {
            return
        }
        do {
            let manifest = try manifestService.loadManifest(at: manifestPath)
            let mappedStatus = finalStatusMapper.mapStatus(manifest)
            switch mappedStatus {
            case .failed:
                _ = transition(
                    to: .failed(
                    AppServiceError(
                        code: .processExitedUnexpectedly,
                        userMessage: "Session ended with a failure.",
                        remediation: "Open details and retry after fixing reported issues.",
                        debugDetail: "manifest status=failed"
                    )
                )
                ,
                    allowedFrom: [.finalizing],
                    action: "loadFinalStatus",
                    invalidUserMessage: "Runtime state changed unexpectedly while mapping final failure status.",
                    invalidRemediation: "Refresh runtime state and reopen the session detail."
                )
            case .ok, .degraded, .pending:
                _ = transition(
                    to: .completed,
                    allowedFrom: [.finalizing],
                    action: "loadFinalStatus",
                    invalidUserMessage: "Runtime state changed unexpectedly while finalizing session status.",
                    invalidRemediation: "Refresh runtime state and reopen the session detail."
                )
            }
        } catch let serviceError as AppServiceError {
            _ = transition(
                to: .failed(serviceError),
                allowedFrom: [.finalizing],
                action: "loadFinalStatus",
                invalidUserMessage: "Runtime state changed unexpectedly while reading final status artifacts.",
                invalidRemediation: "Retry final status load after refreshing the session."
            )
        } catch {
            _ = transition(
                to: .failed(
                AppServiceError(
                    code: .manifestInvalid,
                    userMessage: "Session summary is unavailable.",
                    remediation: "Re-open the session or run replay on session.jsonl.",
                    debugDetail: String(describing: error)
                )
            )
            ,
                allowedFrom: [.finalizing],
                action: "loadFinalStatus",
                invalidUserMessage: "Runtime state changed unexpectedly while reading final status artifacts.",
                invalidRemediation: "Retry final status load after refreshing the session."
            )
        }
    }

    private enum RunPhase: String {
        case idle
        case preparing
        case running
        case stopping
        case finalizing
        case completed
        case failed
    }

    private func currentPhase(for runState: RunState) -> RunPhase {
        switch runState {
        case .idle:
            return .idle
        case .preparing:
            return .preparing
        case .running:
            return .running
        case .stopping:
            return .stopping
        case .finalizing:
            return .finalizing
        case .completed:
            return .completed
        case .failed:
            return .failed
        }
    }

    @discardableResult
    private func transition(
        to next: RunState,
        allowedFrom: Set<RunPhase>,
        action: String,
        invalidUserMessage: String,
        invalidRemediation: String
    ) -> Bool {
        let phase = currentPhase(for: state)
        guard allowedFrom.contains(phase) else {
            lastRejectedActionError = AppServiceError(
                code: .invalidInput,
                userMessage: invalidUserMessage,
                remediation: invalidRemediation,
                debugDetail: "action=\(action), state=\(phase.rawValue)"
            )
            return false
        }

        state = next
        lastRejectedActionError = nil
        return true
    }

    private func rejectAction(action: String, userMessage: String, remediation: String) {
        let phase = currentPhase(for: state)
        lastRejectedActionError = AppServiceError(
            code: .invalidInput,
            userMessage: userMessage,
            remediation: remediation,
            debugDetail: "action=\(action), state=\(phase.rawValue)"
        )
    }
}
