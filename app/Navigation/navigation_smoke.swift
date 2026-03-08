import Foundation

@MainActor
private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("navigation_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

@MainActor
private func runSmoke() {
    let coordinator = AppNavigationCoordinator(firstRun: true)
    check(coordinator.state.root == .onboarding, "expected onboarding root for first run")

    coordinator.dispatch(.finishOnboarding)
    check(coordinator.state.root == .mainRuntime, "finishOnboarding should transition to mainRuntime")

    coordinator.dispatch(.deepLink(.sessionDetail(sessionID: "sess-001")))
    check(coordinator.state.root == .sessions, "session deep link should route to sessions root")
    check(coordinator.state.sessionsPath == [.list, .detail(sessionID: "sess-001")], "session deep link path mismatch")

    coordinator.dispatch(.back)
    check(coordinator.state.sessionsPath == [.list], "back from detail should pop to sessions list")

    coordinator.dispatch(.back)
    check(coordinator.state.root == .mainRuntime, "back from sessions list should return to main runtime")

    coordinator.dispatch(.showRuntimeError(errorCode: .runtimeUnavailable))
    check(coordinator.state.overlay == .runtimeError(code: .runtimeUnavailable), "runtime error overlay missing")

    coordinator.dispatch(.back)
    check(coordinator.state.overlay == nil, "back with overlay should dismiss overlay first")
    check(coordinator.state.root == .mainRuntime, "overlay dismissal should keep current root")

    coordinator.dispatch(.deepLink(.recovery(errorCode: .permissionDenied)))
    check(coordinator.state.root == .recovery, "recovery deep link should set recovery root")
    check(coordinator.state.recovery == .permissionRecovery, "permissionDenied should map to permission recovery")

    coordinator.dispatch(.back)
    check(coordinator.state.root == .mainRuntime, "back from recovery should return to main runtime")
}

@main
struct NavigationSmokeMain {
    @MainActor
    static func main() {
        runSmoke()
        print("navigation_smoke: PASS")
    }
}
