import Foundation

public enum AppRootRoute: String, Codable, Sendable {
    case onboarding
    case mainRuntime = "main_runtime"
    case sessions
    case recovery
}

public enum SessionsRoute: Equatable, Sendable {
    case list
    case detail(sessionID: String)
}

public enum RuntimeOverlayRoute: Equatable, Sendable {
    case sessionSummary(sessionID: String)
    case runtimeError(code: AppServiceErrorCode)
}

public enum RecoveryRoute: Equatable, Sendable {
    case permissionRecovery
    case modelRecovery
    case runtimeRecovery

    public init(errorCode: AppServiceErrorCode) {
        switch errorCode {
        case .permissionDenied:
            self = .permissionRecovery
        case .modelUnavailable:
            self = .modelRecovery
        default:
            self = .runtimeRecovery
        }
    }
}

public struct NavigationState: Equatable, Sendable {
    public var root: AppRootRoute
    public var sessionsPath: [SessionsRoute]
    public var recovery: RecoveryRoute?
    public var overlay: RuntimeOverlayRoute?

    public init(
        root: AppRootRoute,
        sessionsPath: [SessionsRoute] = [],
        recovery: RecoveryRoute? = nil,
        overlay: RuntimeOverlayRoute? = nil
    ) {
        self.root = root
        self.sessionsPath = sessionsPath
        self.recovery = recovery
        self.overlay = overlay
    }
}

public enum DeepLinkTarget: Equatable, Sendable {
    case onboarding
    case mainRuntime
    case sessionsList
    case sessionDetail(sessionID: String)
    case recovery(errorCode: AppServiceErrorCode)
}

public enum NavigationIntent: Equatable, Sendable {
    case finishOnboarding
    case openMainRuntime
    case openSessions
    case openSessionDetail(sessionID: String)
    case openRecovery(errorCode: AppServiceErrorCode)
    case showSessionSummary(sessionID: String)
    case showRuntimeError(errorCode: AppServiceErrorCode)
    case dismissOverlay
    case deepLink(DeepLinkTarget)
    case back
    case resetToRoot
}
