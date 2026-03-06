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

    public enum RecoveryAction: String, Equatable, Hashable, Sendable {
        case resumeSession = "resume_session"
        case safeFinalize = "safe_finalize"
        case retryStop = "retry_stop"
        case retryFinalize = "retry_finalize"
        case openSessionArtifacts = "open_session_artifacts"
        case runPreflight = "run_preflight"
        case startNewSession = "start_new_session"
    }

    public enum InterruptionRecoveryClassification: String, Equatable, Sendable {
        case recoverableInterruption = "recoverable_interruption"
    }

    public struct InterruptionRecoveryContext: Equatable, Sendable {
        public var classification: InterruptionRecoveryClassification
        public var sessionRoot: URL
        public var summary: String
        public var guidance: String
        public var actions: [RecoveryAction]

        public init(
            classification: InterruptionRecoveryClassification,
            sessionRoot: URL,
            summary: String,
            guidance: String,
            actions: [RecoveryAction]
        ) {
            self.classification = classification
            self.sessionRoot = sessionRoot
            self.summary = summary
            self.guidance = guidance
            self.actions = actions
        }
    }

    public private(set) var state: RunState = .idle
    public private(set) var lastRejectedActionError: AppServiceError?
    public private(set) var suggestedRecoveryActions: [RecoveryAction] = []
    public private(set) var interruptionRecoveryContext: InterruptionRecoveryContext?

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
            id: "resume_interrupted_session",
            label: "Resume interrupted session",
            hint: "Restarts capture in the interrupted session folder."
        ),
        AccessibilityElementDescriptor(
            id: "safe_finalize_session",
            label: "Safe finalize session",
            hint: "Finalizes available artifacts after an interruption."
        ),
        AccessibilityElementDescriptor(
            id: "runtime_status",
            label: "Runtime status",
            hint: "Announces launch, running, and failure state changes."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: [
            "start_live_transcribe",
            "stop_live_transcribe",
            "resume_interrupted_session",
            "safe_finalize_session",
            "runtime_status",
        ]
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
        KeyboardShortcutDescriptor(
            id: "resume_interrupted_shortcut",
            key: "r",
            modifiers: ["command"],
            actionSummary: "Resume an interrupted session."
        ),
        KeyboardShortcutDescriptor(
            id: "safe_finalize_shortcut",
            key: "f",
            modifiers: ["command", "shift"],
            actionSummary: "Run safe finalization for an interrupted session."
        ),
    ]

    private let runtimeService: RuntimeService
    private let manifestService: ManifestService
    private let modelService: ModelResolutionService
    private let finalStatusMapper: ManifestFinalStatusMapper
    private let finalizationTimeoutSeconds: TimeInterval
    private let finalizationPollIntervalNanoseconds: UInt64
    private let now: @Sendable () -> Date
    private let sleep: @Sendable (UInt64) async -> Void
    private var activeSessionRoot: URL?

    public init(
        runtimeService: RuntimeService,
        manifestService: ManifestService,
        modelService: ModelResolutionService,
        finalStatusMapper: ManifestFinalStatusMapper = ManifestFinalStatusMapper(),
        finalizationTimeoutSeconds: TimeInterval = 15,
        finalizationPollIntervalNanoseconds: UInt64 = 250_000_000,
        now: @escaping @Sendable () -> Date = { Date() },
        sleep: @escaping @Sendable (UInt64) async -> Void = { nanoseconds in
            try? await Task.sleep(nanoseconds: nanoseconds)
        }
    ) {
        self.runtimeService = runtimeService
        self.manifestService = manifestService
        self.modelService = modelService
        self.finalStatusMapper = finalStatusMapper
        self.finalizationTimeoutSeconds = max(0.5, finalizationTimeoutSeconds)
        self.finalizationPollIntervalNanoseconds = max(10_000_000, finalizationPollIntervalNanoseconds)
        self.now = now
        self.sleep = sleep
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
            let resolvedModel = try modelService.resolveModel(
                ModelResolutionRequest(
                    explicitModelPath: explicitModelPath,
                    backend: "whispercpp"
                )
            )
            let result = try await runtimeService.startSession(
                request: RuntimeStartRequest(
                    mode: .live,
                    outputRoot: outputRoot,
                    modelPath: resolvedModel.resolvedPath
                )
            )
            activeSessionRoot = result.sessionRoot
            _ = transition(
                to: .running(processID: result.processIdentifier),
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly before launch completed.",
                invalidRemediation: "Reset the run state and try starting again."
            )
        } catch let serviceError as AppServiceError {
            _ = transitionToFailure(
                serviceError,
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly while handling launch failure.",
                invalidRemediation: "Reset the run state and retry launch.",
                recoveryActions: suggestedActions(for: serviceError)
            )
        } catch {
            _ = transitionToFailure(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not start session.",
                    remediation: "Try again. If this keeps happening, run preflight diagnostics first.",
                    debugDetail: String(describing: error)
                ),
                allowedFrom: [.preparing],
                action: "startLive",
                invalidUserMessage: "Runtime state changed unexpectedly while handling launch failure.",
                invalidRemediation: "Reset the run state and retry launch.",
                recoveryActions: [.runPreflight, .startNewSession]
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
            guard transition(
                to: .finalizing,
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly during stop finalization.",
                invalidRemediation: "Load final status to recover state."
            ) else {
                return
            }
            await finalizeStopBounded()
        } catch let serviceError as AppServiceError {
            _ = transitionToFailure(
                serviceError,
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly while handling stop failure.",
                invalidRemediation: "Refresh runtime state and retry control action.",
                recoveryActions: interruptionRecoveryActions(
                    for: serviceError,
                    fallback: [.retryStop, .openSessionArtifacts]
                )
            )
        } catch {
            _ = transitionToFailure(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not stop session cleanly.",
                    remediation: "Wait a few seconds and try Stop again.",
                    debugDetail: String(describing: error)
                ),
                allowedFrom: [.stopping],
                action: "stopCurrentRun",
                invalidUserMessage: "Runtime state changed unexpectedly while handling stop failure.",
                invalidRemediation: "Refresh runtime state and retry control action.",
                recoveryActions: [.retryStop, .openSessionArtifacts]
            )
        }
    }

    public func loadFinalStatus(manifestPath: URL) {
        activeSessionRoot = manifestPath.deletingLastPathComponent()
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
            applyManifestFinalStatus(manifest, action: "loadFinalStatus")
        } catch let serviceError as AppServiceError {
            _ = transitionToFailure(
                serviceError,
                allowedFrom: [.finalizing],
                action: "loadFinalStatus",
                invalidUserMessage: "Runtime state changed unexpectedly while reading final status artifacts.",
                invalidRemediation: "Retry final status load after refreshing the session.",
                recoveryActions: suggestedActions(for: serviceError)
            )
        } catch {
            _ = transitionToFailure(
                AppServiceError(
                    code: .manifestInvalid,
                    userMessage: "Session summary is unavailable.",
                    remediation: "Re-open the session or run replay on session.jsonl.",
                    debugDetail: String(describing: error)
                ),
                allowedFrom: [.finalizing],
                action: "loadFinalStatus",
                invalidUserMessage: "Runtime state changed unexpectedly while reading final status artifacts.",
                invalidRemediation: "Retry final status load after refreshing the session.",
                recoveryActions: [.openSessionArtifacts, .retryFinalize]
            )
        }
    }

    public func resumeInterruptedSession(explicitModelPath: URL? = nil) async {
        guard let sessionRoot = interruptionRecoveryContext?.sessionRoot ?? activeSessionRoot else {
            rejectAction(
                action: "resumeInterruptedSession",
                userMessage: "No interrupted session is available to resume.",
                remediation: "Start a new session or open session artifacts for manual recovery."
            )
            return
        }
        await startLive(outputRoot: sessionRoot, explicitModelPath: explicitModelPath)
    }

    public func safeFinalizeInterruptedSession() {
        guard let sessionRoot = interruptionRecoveryContext?.sessionRoot ?? activeSessionRoot else {
            rejectAction(
                action: "safeFinalizeInterruptedSession",
                userMessage: "No interrupted session is available to finalize.",
                remediation: "Open session artifacts and verify a manifest exists before retrying."
            )
            return
        }
        loadFinalStatus(manifestPath: sessionRoot.appendingPathComponent("session.manifest.json"))
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
        if case .completed = next {
            suggestedRecoveryActions = []
            activeSessionRoot = nil
            interruptionRecoveryContext = nil
        } else if case .preparing = next {
            interruptionRecoveryContext = nil
        } else if case .running = next {
            suggestedRecoveryActions = []
            interruptionRecoveryContext = nil
        }
        return true
    }

    private func finalizeStopBounded() async {
        guard let sessionRoot = activeSessionRoot else {
            _ = transitionToFailure(
                AppServiceError(
                    code: .artifactMissing,
                    userMessage: "Session artifacts could not be located for finalization.",
                    remediation: "Open Sessions and inspect the latest run folder before retrying."
                ),
                allowedFrom: [.finalizing],
                action: "stopCurrentRun.finalize",
                invalidUserMessage: "Finalization could not start because runtime state changed unexpectedly.",
                invalidRemediation: "Refresh runtime state and retry finalization.",
                recoveryActions: [.openSessionArtifacts, .startNewSession]
            )
            return
        }

        let manifestPath = sessionRoot.appendingPathComponent("session.manifest.json")
        let deadline = now().addingTimeInterval(finalizationTimeoutSeconds)

        while now() <= deadline {
            do {
                let manifest = try manifestService.loadManifest(at: manifestPath)
                applyManifestFinalStatus(manifest, action: "stopCurrentRun.finalize")
                return
            } catch let serviceError as AppServiceError {
                if isTransientFinalizationError(serviceError), now() < deadline {
                    await sleep(finalizationPollIntervalNanoseconds)
                    continue
                }
                _ = transitionToFailure(
                    serviceError,
                    allowedFrom: [.finalizing],
                    action: "stopCurrentRun.finalize",
                    invalidUserMessage: "Runtime state changed unexpectedly while finalizing stop.",
                    invalidRemediation: "Refresh runtime state and retry finalization.",
                    recoveryActions: suggestedActions(for: serviceError)
                )
                return
            } catch {
                let wrapped = AppServiceError(
                    code: .manifestInvalid,
                    userMessage: "Final status artifacts are malformed.",
                    remediation: "Open session details and inspect generated artifacts before retrying.",
                    debugDetail: String(describing: error)
                )
                _ = transitionToFailure(
                    wrapped,
                    allowedFrom: [.finalizing],
                    action: "stopCurrentRun.finalize",
                    invalidUserMessage: "Runtime state changed unexpectedly while finalizing stop.",
                    invalidRemediation: "Refresh runtime state and retry finalization.",
                    recoveryActions: [.openSessionArtifacts, .retryFinalize]
                )
                return
            }
        }

        // One final read attempt reduces deadline-edge races where the manifest lands
        // right after the loop's last transient read failure.
        if let manifest = try? manifestService.loadManifest(at: manifestPath) {
            applyManifestFinalStatus(manifest, action: "stopCurrentRun.finalize")
            return
        }

        let diagnostics = finalizationTimeoutDiagnostics(
            sessionRoot: sessionRoot,
            manifestPath: manifestPath
        )
        _ = transitionToFailure(
            AppServiceError(
                code: .timeout,
                userMessage: "Session finalization timed out.",
                remediation: "Open session details to inspect artifacts, then retry finalization.",
                debugDetail: "timeout_seconds=\(finalizationTimeoutSeconds), \(diagnostics)"
            ),
            allowedFrom: [.finalizing],
            action: "stopCurrentRun.finalize",
            invalidUserMessage: "Finalization timed out after runtime state changed unexpectedly.",
            invalidRemediation: "Refresh runtime state and retry finalization.",
            recoveryActions: [.safeFinalize, .retryFinalize, .openSessionArtifacts, .startNewSession]
        )
    }

    private func finalizationTimeoutDiagnostics(sessionRoot: URL, manifestPath: URL) -> String {
        let fileManager = FileManager.default
        let jsonlPath = sessionRoot.appendingPathComponent("session.jsonl")
        let wavPath = sessionRoot.appendingPathComponent("session.wav")
        let stderrPath = sessionRoot.appendingPathComponent("runtime.stderr.log")

        return [
            "session_root=\(sessionRoot.path)",
            "manifest_path=\(manifestPath.path)",
            "manifest_exists=\(fileManager.fileExists(atPath: manifestPath.path))",
            "jsonl_exists=\(fileManager.fileExists(atPath: jsonlPath.path))",
            "wav_exists=\(fileManager.fileExists(atPath: wavPath.path))",
            "stderr_exists=\(fileManager.fileExists(atPath: stderrPath.path))",
        ].joined(separator: ", ")
    }

    private func applyManifestFinalStatus(_ manifest: SessionManifestDTO, action: String) {
        let mappedStatus = finalStatusMapper.mapStatus(manifest)
        switch mappedStatus {
        case .failed:
            let interruptedFailure = AppServiceError(
                code: .processExitedUnexpectedly,
                userMessage: "Session ended with a failure.",
                remediation: "Open details and retry after fixing reported issues.",
                debugDetail: "manifest status=failed"
            )
            _ = transitionToFailure(
                interruptedFailure,
                allowedFrom: [.finalizing],
                action: action,
                invalidUserMessage: "Runtime state changed unexpectedly while mapping final failure status.",
                invalidRemediation: "Refresh runtime state and reopen the session detail.",
                recoveryActions: interruptionRecoveryActions(
                    for: interruptedFailure,
                    fallback: [.openSessionArtifacts, .startNewSession]
                )
            )
        case .ok, .degraded, .pending:
            _ = transition(
                to: .completed,
                allowedFrom: [.finalizing],
                action: action,
                invalidUserMessage: "Runtime state changed unexpectedly while finalizing session status.",
                invalidRemediation: "Refresh runtime state and reopen the session detail."
            )
        }
    }

    private func transitionToFailure(
        _ error: AppServiceError,
        allowedFrom: Set<RunPhase>,
        action: String,
        invalidUserMessage: String,
        invalidRemediation: String,
        recoveryActions: [RecoveryAction]
    ) -> Bool {
        let transitioned = transition(
            to: .failed(error),
            allowedFrom: allowedFrom,
            action: action,
            invalidUserMessage: invalidUserMessage,
            invalidRemediation: invalidRemediation
        )
        if transitioned {
            let normalizedActions = normalizedRecoveryActions(recoveryActions)
            suggestedRecoveryActions = normalizedActions
            interruptionRecoveryContext = makeInterruptionRecoveryContext(
                error: error,
                actions: normalizedActions
            )
        }
        return transitioned
    }

    private func isTransientFinalizationError(_ error: AppServiceError) -> Bool {
        switch error.code {
        case .artifactMissing, .ioFailure, .manifestInvalid:
            return true
        default:
            return false
        }
    }

    private func suggestedActions(for error: AppServiceError) -> [RecoveryAction] {
        switch error.code {
        case .timeout:
            return [.safeFinalize, .retryFinalize, .openSessionArtifacts, .startNewSession]
        case .processExitedUnexpectedly:
            return [.resumeSession, .safeFinalize, .openSessionArtifacts, .startNewSession]
        case .runtimeUnavailable, .processLaunchFailed, .preflightFailed, .permissionDenied, .modelUnavailable:
            return [.runPreflight, .startNewSession]
        case .manifestInvalid, .artifactMissing, .jsonlCorrupt, .ioFailure:
            return [.openSessionArtifacts, .retryFinalize]
        case .invalidInput:
            return [.startNewSession]
        case .unknown:
            return [.startNewSession]
        }
    }

    private func interruptionRecoveryActions(
        for error: AppServiceError,
        fallback: [RecoveryAction]
    ) -> [RecoveryAction] {
        guard isRecoverableInterruption(error) else {
            return fallback
        }
        return normalizedRecoveryActions([.resumeSession, .safeFinalize] + fallback + [.startNewSession])
    }

    private func isRecoverableInterruption(_ error: AppServiceError) -> Bool {
        switch error.code {
        case .processExitedUnexpectedly, .timeout:
            return true
        default:
            return false
        }
    }

    private func makeInterruptionRecoveryContext(
        error: AppServiceError,
        actions: [RecoveryAction]
    ) -> InterruptionRecoveryContext? {
        guard isRecoverableInterruption(error), let sessionRoot = activeSessionRoot else {
            return nil
        }
        return InterruptionRecoveryContext(
            classification: .recoverableInterruption,
            sessionRoot: sessionRoot,
            summary: "Session was interrupted before clean finalization.",
            guidance: "Choose Resume to continue this session or Safe Finalize to preserve partial artifacts for review.",
            actions: normalizedRecoveryActions([.resumeSession, .safeFinalize] + actions)
        )
    }

    private func normalizedRecoveryActions(_ actions: [RecoveryAction]) -> [RecoveryAction] {
        var ordered = [RecoveryAction]()
        var seen = Set<RecoveryAction>()
        for action in actions where !seen.contains(action) {
            seen.insert(action)
            ordered.append(action)
        }
        return ordered
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
