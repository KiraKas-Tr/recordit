import Foundation

@MainActor
public final class PreflightViewModel {
    public enum State: Equatable {
        case idle
        case running
        case completed(PreflightManifestEnvelopeDTO)
        case failed(AppServiceError)
    }

    public private(set) var state: State = .idle
    public private(set) var gatingEvaluation: PreflightGatingEvaluation?
    public private(set) var warningAcknowledged = false

    public static let accessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "run_preflight",
            label: "Run preflight checks",
            hint: "Runs required checks for model, capture permissions, and output paths."
        ),
        AccessibilityElementDescriptor(
            id: "preflight_results",
            label: "Preflight results",
            hint: "Review failed and warning checks before continuing."
        ),
        AccessibilityElementDescriptor(
            id: "acknowledge_warnings",
            label: "Acknowledge warnings",
            hint: "Required before continuing when warning checks are present."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: ["run_preflight", "preflight_results", "acknowledge_warnings"]
    )

    public static let keyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "run_preflight_shortcut",
            key: "return",
            modifiers: ["command", "shift"],
            actionSummary: "Run preflight checks."
        ),
    ]

    private let runner: RecorditPreflightRunner
    private let gatingPolicy: PreflightGatingPolicy

    public init(
        runner: RecorditPreflightRunner = RecorditPreflightRunner(),
        gatingPolicy: PreflightGatingPolicy = PreflightGatingPolicy()
    ) {
        self.runner = runner
        self.gatingPolicy = gatingPolicy
    }

    public var canProceedToLiveTranscribe: Bool {
        guard let evaluation = gatingEvaluation else {
            return false
        }
        return evaluation.canProceed(acknowledgingWarnings: warningAcknowledged)
    }

    public var requiresWarningAcknowledgement: Bool {
        guard let evaluation = gatingEvaluation else {
            return false
        }
        return evaluation.requiresWarningAcknowledgement && !warningAcknowledged
    }

    public func acknowledgeWarningsForLiveTranscribe() {
        warningAcknowledged = true
    }

    public func runLivePreflight() {
        state = .running
        gatingEvaluation = nil
        warningAcknowledged = false
        do {
            let envelope = try runner.runLivePreflight()
            gatingEvaluation = gatingPolicy.evaluate(envelope)
            state = .completed(envelope)
        } catch let serviceError as AppServiceError {
            gatingEvaluation = nil
            state = .failed(serviceError)
        } catch {
            gatingEvaluation = nil
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Preflight could not complete.",
                    remediation: "Retry preflight and inspect command diagnostics.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }
}
