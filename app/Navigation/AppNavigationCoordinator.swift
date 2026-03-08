import Foundation

@MainActor
public final class AppNavigationCoordinator {
    public private(set) var state: NavigationState

    public init(firstRun: Bool) {
        self.state = NavigationState(root: firstRun ? .onboarding : .mainRuntime)
    }

    public func dispatch(_ intent: NavigationIntent) {
        switch intent {
        case .finishOnboarding:
            state.root = .mainRuntime
            state.recovery = nil
            state.overlay = nil
        case .openMainRuntime:
            state.root = .mainRuntime
            state.recovery = nil
        case .openSessions:
            state.root = .sessions
            state.sessionsPath = [.list]
            state.recovery = nil
        case let .openSessionDetail(sessionID):
            state.root = .sessions
            state.recovery = nil
            state.sessionsPath = [.list, .detail(sessionID: sessionID)]
        case let .openRecovery(errorCode):
            state.root = .recovery
            state.recovery = RecoveryRoute(errorCode: errorCode)
            state.overlay = nil
        case let .showSessionSummary(sessionID):
            state.overlay = .sessionSummary(sessionID: sessionID)
        case let .showRuntimeError(errorCode):
            state.overlay = .runtimeError(code: errorCode)
        case .dismissOverlay:
            state.overlay = nil
        case let .deepLink(target):
            applyDeepLink(target)
        case .back:
            applyBackNavigation()
        case .resetToRoot:
            resetPathsForRoot()
            state.overlay = nil
        }
    }

    public var canNavigateBack: Bool {
        if state.overlay != nil {
            return true
        }
        switch state.root {
        case .sessions:
            return state.sessionsPath.count > 1
        case .recovery:
            return true
        case .onboarding, .mainRuntime:
            return false
        }
    }

    private func applyDeepLink(_ target: DeepLinkTarget) {
        switch target {
        case .onboarding:
            state = NavigationState(root: .onboarding)
        case .mainRuntime:
            state = NavigationState(root: .mainRuntime)
        case .sessionsList:
            state = NavigationState(root: .sessions, sessionsPath: [.list])
        case let .sessionDetail(sessionID):
            state = NavigationState(
                root: .sessions,
                sessionsPath: [.list, .detail(sessionID: sessionID)]
            )
        case let .recovery(errorCode):
            state = NavigationState(
                root: .recovery,
                sessionsPath: [],
                recovery: RecoveryRoute(errorCode: errorCode),
                overlay: nil
            )
        }
    }

    private func applyBackNavigation() {
        if state.overlay != nil {
            state.overlay = nil
            return
        }
        switch state.root {
        case .sessions:
            if state.sessionsPath.count > 1 {
                state.sessionsPath.removeLast()
            } else {
                state.root = .mainRuntime
                state.sessionsPath = []
            }
        case .recovery:
            state.root = .mainRuntime
            state.recovery = nil
        case .onboarding, .mainRuntime:
            break
        }
    }

    private func resetPathsForRoot() {
        switch state.root {
        case .sessions:
            state.sessionsPath = [.list]
            state.recovery = nil
        case .recovery:
            state.sessionsPath = []
        case .onboarding, .mainRuntime:
            state.sessionsPath = []
            state.recovery = nil
        }
    }
}
