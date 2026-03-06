import Foundation

@MainActor
public final class AppShellViewModel {
    public private(set) var navigationState: NavigationState
    public let navigationCoordinator: AppNavigationCoordinator
    public private(set) var onboardingGateFailure: AppServiceError?
    public private(set) var startupRuntimeReadinessReport: RuntimeBinaryReadinessReport
    public private(set) var startupRuntimeReadinessFailure: AppServiceError?

    private let onboardingCompletionStore: any OnboardingCompletionStore
    private let runtimeReadinessChecker: any RuntimeBinaryReadinessChecking

    public init(
        firstRun: Bool? = nil,
        onboardingCompletionStore: any OnboardingCompletionStore = UserDefaultsOnboardingCompletionStore(),
        runtimeReadinessChecker: any RuntimeBinaryReadinessChecking = RuntimeBinaryReadinessService()
    ) {
        self.onboardingCompletionStore = onboardingCompletionStore
        self.runtimeReadinessChecker = runtimeReadinessChecker
        let readinessReport = runtimeReadinessChecker.evaluateStartupReadiness()
        startupRuntimeReadinessReport = readinessReport
        startupRuntimeReadinessFailure = runtimeReadinessChecker.startupBlockingError(from: readinessReport)
        let resolvedFirstRun = firstRun ?? !onboardingCompletionStore.isOnboardingComplete()
        let coordinator = AppNavigationCoordinator(firstRun: resolvedFirstRun)
        self.navigationCoordinator = coordinator
        self.navigationState = coordinator.state

        if !resolvedFirstRun, startupRuntimeReadinessFailure != nil {
            send(.openRecovery(errorCode: .runtimeUnavailable))
        }
    }

    public func send(_ intent: NavigationIntent) {
        navigationCoordinator.dispatch(intent)
        navigationState = navigationCoordinator.state
    }

    public func completeOnboardingIfReady(
        modelSetup: ModelSetupViewModel,
        preflight: PreflightViewModel
    ) -> Bool {
        guard refreshStartupRuntimeReadiness() else {
            onboardingGateFailure = startupRuntimeReadinessFailure
            return false
        }

        guard modelSetup.canStartLiveTranscribe else {
            onboardingGateFailure = AppServiceError(
                code: .modelUnavailable,
                userMessage: "Select a valid local model before finishing setup.",
                remediation: "Choose a model path that matches the selected backend."
            )
            return false
        }

        guard preflight.canProceedToLiveTranscribe else {
            onboardingGateFailure = Self.preflightGateFailure(for: preflight)
            return false
        }

        onboardingCompletionStore.markOnboardingComplete()
        onboardingGateFailure = nil
        send(.finishOnboarding)
        return true
    }

    @discardableResult
    public func refreshStartupRuntimeReadiness() -> Bool {
        let report = runtimeReadinessChecker.evaluateStartupReadiness()
        startupRuntimeReadinessReport = report
        startupRuntimeReadinessFailure = runtimeReadinessChecker.startupBlockingError(from: report)
        return startupRuntimeReadinessFailure == nil
    }

    public func resetOnboardingCompletion() {
        onboardingCompletionStore.resetOnboardingCompletion()
        onboardingGateFailure = nil
        send(.deepLink(.onboarding))
    }

    public var isOnboardingComplete: Bool {
        onboardingCompletionStore.isOnboardingComplete()
    }

    public var activeRoot: AppRootRoute {
        navigationState.root
    }

    public var activeSessionDetailID: String? {
        guard case let .detail(sessionID) = navigationState.sessionsPath.last else {
            return nil
        }
        return sessionID
    }

    private static func preflightGateFailure(for preflight: PreflightViewModel) -> AppServiceError {
        let fallbackRemediationSuffix = preflight.canOfferRecordOnlyFallback
            ? " Record Only remains available while Live Transcribe is blocked."
            : ""

        switch preflight.primaryBlockingDomain {
        case .tccCapture:
            return AppServiceError(
                code: .permissionDenied,
                userMessage: "Live Transcribe is blocked by capture permission readiness.",
                remediation: "Grant Screen Recording and Microphone access, ensure an active display, then rerun preflight."
            )
        case .backendModel:
            return AppServiceError(
                code: .modelUnavailable,
                userMessage: "Live Transcribe is blocked by backend/model readiness.",
                remediation: "Fix model path/backend compatibility and rerun preflight.\(fallbackRemediationSuffix)"
            )
        case .runtimePreflight, .backendRuntime:
            return AppServiceError(
                code: .preflightFailed,
                userMessage: "Live Transcribe is blocked by runtime preflight checks.",
                remediation: "Resolve failed runtime checks, rerun preflight, and review diagnostics.\(fallbackRemediationSuffix)"
            )
        case .diagnosticOnly, .unknown, .none:
            return AppServiceError(
                code: .preflightFailed,
                userMessage: "Run preflight checks before finishing setup.",
                remediation: "Resolve failed checks and acknowledge warnings before continuing."
            )
        }
    }
}
