import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("runtime_stop_finalization_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private actor StubRuntimeService: RuntimeService {
    func startSession(request: RuntimeStartRequest) async throws -> RuntimeLaunchResult {
        RuntimeLaunchResult(
            processIdentifier: 4242,
            sessionRoot: request.outputRoot,
            startedAt: Date()
        )
    }

    func controlSession(processIdentifier _: Int32, action _: RuntimeControlAction) async throws -> RuntimeControlResult {
        RuntimeControlResult(accepted: true, detail: "stopped")
    }
}

private actor CrashOnStopRuntimeService: RuntimeService {
    private(set) var startInvocations = 0

    func startSession(request: RuntimeStartRequest) async throws -> RuntimeLaunchResult {
        startInvocations += 1
        return RuntimeLaunchResult(
            processIdentifier: 5000 + Int32(startInvocations),
            sessionRoot: request.outputRoot,
            startedAt: Date()
        )
    }

    func controlSession(processIdentifier _: Int32, action _: RuntimeControlAction) async throws -> RuntimeControlResult {
        throw AppServiceError(
            code: .processExitedUnexpectedly,
            userMessage: "Session was interrupted unexpectedly.",
            remediation: "Use Resume or Safe Finalize to preserve captured artifacts."
        )
    }

    func startCount() -> Int {
        startInvocations
    }
}

private struct StaticModelService: ModelResolutionService {
    func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        _ = request
        return ResolvedModelDTO(
            resolvedPath: URL(fileURLWithPath: "/tmp/model.bin"),
            source: "smoke",
            checksumSHA256: nil,
            checksumStatus: "available"
        )
    }
}

private struct DelayThenSuccessManifestService: ManifestService {
    private let startTime: Date
    private let readyAfterSeconds: TimeInterval
    private let manifest: SessionManifestDTO

    init(readyAfterSeconds: TimeInterval, manifest: SessionManifestDTO) {
        startTime = Date()
        self.readyAfterSeconds = readyAfterSeconds
        self.manifest = manifest
    }

    func loadManifest(at _: URL) throws -> SessionManifestDTO {
        if Date().timeIntervalSince(startTime) < readyAfterSeconds {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "manifest not ready",
                remediation: "retry"
            )
        }
        return manifest
    }
}

private struct AlwaysMissingManifestService: ManifestService {
    func loadManifest(at _: URL) throws -> SessionManifestDTO {
        throw AppServiceError(
            code: .artifactMissing,
            userMessage: "manifest missing",
            remediation: "retry"
        )
    }
}

private struct FailedManifestService: ManifestService {
    let manifest: SessionManifestDTO

    func loadManifest(at _: URL) throws -> SessionManifestDTO {
        manifest
    }
}

private func makeManifest(status: String, trustNoticeCount: Int = 0) -> SessionManifestDTO {
    SessionManifestDTO(
        sessionID: "stop-smoke",
        status: status,
        runtimeMode: "live",
        trustNoticeCount: trustNoticeCount,
        artifacts: SessionArtifactsDTO(
            wavPath: URL(fileURLWithPath: "/tmp/stop-smoke.wav"),
            jsonlPath: URL(fileURLWithPath: "/tmp/stop-smoke.jsonl"),
            manifestPath: URL(fileURLWithPath: "/tmp/stop-smoke.manifest.json")
        )
    )
}

@MainActor
private func runSmoke() async {
    let runtime = StubRuntimeService()
    let model = StaticModelService()

    let eventuallySuccess = RuntimeViewModel(
        runtimeService: runtime,
        manifestService: DelayThenSuccessManifestService(
            readyAfterSeconds: 0.03,
            manifest: makeManifest(status: "ok")
        ),
        modelService: model,
        finalizationTimeoutSeconds: 1,
        finalizationPollIntervalNanoseconds: 10_000_000
    )
    await eventuallySuccess.startLive(outputRoot: URL(fileURLWithPath: "/tmp/finalize-success"), explicitModelPath: nil)
    await eventuallySuccess.stopCurrentRun()
    check(eventuallySuccess.state == .completed, "bounded finalization should complete once manifest appears")
    check(eventuallySuccess.suggestedRecoveryActions.isEmpty, "successful bounded finalization should not suggest recovery actions")

    let timeoutFailure = RuntimeViewModel(
        runtimeService: runtime,
        manifestService: AlwaysMissingManifestService(),
        modelService: model,
        finalizationTimeoutSeconds: 0.12,
        finalizationPollIntervalNanoseconds: 10_000_000
    )
    await timeoutFailure.startLive(outputRoot: URL(fileURLWithPath: "/tmp/finalize-timeout"), explicitModelPath: nil)
    await timeoutFailure.stopCurrentRun()
    guard case .failed(let timeoutError) = timeoutFailure.state else {
        check(false, "missing manifest should end in failed timeout state")
        return
    }
    check(timeoutError.code == .timeout, "missing manifest should map to timeout failure")
    check(
        timeoutFailure.suggestedRecoveryActions == [.safeFinalize, .retryFinalize, .openSessionArtifacts, .startNewSession],
        "timeout failure should map to explicit recovery actions"
    )

    let failedManifest = RuntimeViewModel(
        runtimeService: runtime,
        manifestService: FailedManifestService(manifest: makeManifest(status: "failed")),
        modelService: model,
        finalizationTimeoutSeconds: 1,
        finalizationPollIntervalNanoseconds: 10_000_000
    )
    await failedManifest.startLive(outputRoot: URL(fileURLWithPath: "/tmp/finalize-failed"), explicitModelPath: nil)
    await failedManifest.stopCurrentRun()
    guard case .failed(let failedError) = failedManifest.state else {
        check(false, "failed manifest should map to failed state")
        return
    }
    check(failedError.code == .processExitedUnexpectedly, "failed manifest should map to processExitedUnexpectedly")
    check(
        failedManifest.suggestedRecoveryActions == [.resumeSession, .safeFinalize, .openSessionArtifacts, .startNewSession],
        "failed manifest should map to artifact inspection + restart recovery actions"
    )

    let interruptionRuntime = CrashOnStopRuntimeService()
    let interruptionRecovery = RuntimeViewModel(
        runtimeService: interruptionRuntime,
        manifestService: FailedManifestService(manifest: makeManifest(status: "ok")),
        modelService: model,
        finalizationTimeoutSeconds: 1,
        finalizationPollIntervalNanoseconds: 10_000_000
    )
    await interruptionRecovery.startLive(outputRoot: URL(fileURLWithPath: "/tmp/interrupted"), explicitModelPath: nil)
    await interruptionRecovery.stopCurrentRun()
    guard case let .failed(interruptionError) = interruptionRecovery.state else {
        check(false, "interrupted stop should map to failed state")
        return
    }
    check(interruptionError.code == .processExitedUnexpectedly, "interruption failure should classify as processExitedUnexpectedly")
    check(
        interruptionRecovery.suggestedRecoveryActions == [.resumeSession, .safeFinalize, .retryStop, .openSessionArtifacts, .startNewSession],
        "interruption failure should offer resume/safe-finalize recovery actions"
    )
    guard let context = interruptionRecovery.interruptionRecoveryContext else {
        check(false, "interruption failure should surface recoverable interruption context")
        return
    }
    check(context.classification == .recoverableInterruption, "interruption context should be recoverable")
    check(context.sessionRoot.path == "/tmp/interrupted", "interruption context should keep active session root")
    check(context.actions.contains(.resumeSession), "context should include resume action")
    check(context.actions.contains(.safeFinalize), "context should include safe finalize action")
    check(context.guidance.contains("Resume"), "guidance should explain resume action")
    check(context.guidance.contains("Safe Finalize"), "guidance should explain safe finalize action")

    await interruptionRecovery.resumeInterruptedSession()
    guard case .running = interruptionRecovery.state else {
        check(false, "resume action should restart session in interrupted root")
        return
    }
    let resumedStartCount = await interruptionRuntime.startCount()
    check(resumedStartCount == 2, "resume action should launch a second runtime session")

    await interruptionRecovery.stopCurrentRun()
    guard case .failed = interruptionRecovery.state else {
        check(false, "second interrupted stop should return to failed state")
        return
    }
    interruptionRecovery.safeFinalizeInterruptedSession()
    check(interruptionRecovery.state == .completed, "safe finalize should complete via manifest final status load")
    check(interruptionRecovery.interruptionRecoveryContext == nil, "successful safe finalize should clear interruption context")
}

@main
struct RuntimeStopFinalizationSmokeMain {
    static func main() async {
        await runSmoke()
        print("runtime_stop_finalization_smoke: PASS")
    }
}
