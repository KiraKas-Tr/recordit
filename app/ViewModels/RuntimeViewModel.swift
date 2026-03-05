import Foundation

@MainActor
public final class RuntimeViewModel {
    public enum RunState: Equatable {
        case idle
        case launching
        case running(processID: Int32)
        case stopping
        case completed
        case failed(AppServiceError)
    }

    public private(set) var state: RunState = .idle

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
        state = .launching
        do {
            _ = try modelService.resolveModel(ModelResolutionRequest(explicitModelPath: explicitModelPath, backend: "whispercpp"))
            let result = try await runtimeService.startSession(
                request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot, modelPath: explicitModelPath)
            )
            state = .running(processID: result.processIdentifier)
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not start session.",
                    remediation: "Try again. If this keeps happening, run preflight diagnostics first.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func stopCurrentRun() async {
        guard case let .running(processID) = state else { return }
        state = .stopping
        do {
            _ = try await runtimeService.controlSession(processIdentifier: processID, action: .stop)
            state = .completed
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not stop session cleanly.",
                    remediation: "Wait a few seconds and try Stop again.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func loadFinalStatus(manifestPath: URL) {
        do {
            let manifest = try manifestService.loadManifest(at: manifestPath)
            let mappedStatus = finalStatusMapper.mapStatus(manifest)
            switch mappedStatus {
            case .failed:
                state = .failed(
                    AppServiceError(
                        code: .processExitedUnexpectedly,
                        userMessage: "Session ended with a failure.",
                        remediation: "Open details and retry after fixing reported issues.",
                        debugDetail: "manifest status=failed"
                    )
                )
            case .ok, .degraded, .pending:
                state = .completed
            }
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .manifestInvalid,
                    userMessage: "Session summary is unavailable.",
                    remediation: "Re-open the session or run replay on session.jsonl.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }
}
