import Foundation

@MainActor
private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("onboarding_completion_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private final class InMemoryOnboardingCompletionStore: OnboardingCompletionStore {
    private var completed: Bool

    init(completed: Bool = false) {
        self.completed = completed
    }

    func isOnboardingComplete() -> Bool {
        completed
    }

    func markOnboardingComplete() {
        completed = true
    }

    func resetOnboardingCompletion() {
        completed = false
    }
}

private struct StubModelResolutionService: ModelResolutionService {
    let result: Result<ResolvedModelDTO, AppServiceError>

    func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        _ = request
        switch result {
        case let .success(value):
            return value
        case let .failure(error):
            throw error
        }
    }
}

private struct StubCommandRunner: CommandRunning {
    let payload: Data

    func run(
        executable _: String,
        arguments _: [String],
        environment _: [String: String]
    ) throws -> CommandExecutionResult {
        CommandExecutionResult(exitCode: 0, stdout: payload, stderr: Data())
    }
}

private struct StubRuntimeReadinessChecker: RuntimeBinaryReadinessChecking {
    let report: RuntimeBinaryReadinessReport
    let blockingError: AppServiceError?

    func evaluateStartupReadiness() -> RuntimeBinaryReadinessReport {
        report
    }

    func startupBlockingError(from _: RuntimeBinaryReadinessReport) -> AppServiceError? {
        blockingError
    }
}

private func readyRuntimeReadinessChecker() -> StubRuntimeReadinessChecker {
    StubRuntimeReadinessChecker(
        report: RuntimeBinaryReadinessReport(
            checks: [
                RuntimeBinaryReadinessCheck(
                    binaryName: "recordit",
                    overrideEnvKey: RuntimeBinaryResolver.recorditEnvKey,
                    status: .ready,
                    resolvedPath: "/usr/local/bin/recordit",
                    userMessage: "recordit ready",
                    remediation: ""
                ),
                RuntimeBinaryReadinessCheck(
                    binaryName: "sequoia_capture",
                    overrideEnvKey: RuntimeBinaryResolver.sequoiaCaptureEnvKey,
                    status: .ready,
                    resolvedPath: "/usr/local/bin/sequoia_capture",
                    userMessage: "sequoia_capture ready",
                    remediation: ""
                ),
            ]
        ),
        blockingError: nil
    )
}

private func preflightPassPayload() -> Data {
    let payload: [String: Any] = [
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "PASS",
        "config": [
            "out_wav": "/tmp/out.wav",
            "out_jsonl": "/tmp/out.jsonl",
            "out_manifest": "/tmp/out.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "fixture",
            "sample_rate_hz": 48_000,
        ],
        "checks": [
            ["id": ReadinessContractID.modelPath.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
            ["id": ReadinessContractID.screenCaptureAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
            ["id": ReadinessContractID.microphoneAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
        ],
    ]
    return (try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])) ?? Data()
}

private func preflightModelBlockedPayload() -> Data {
    let payload: [String: Any] = [
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "FAIL",
        "config": [
            "out_wav": "/tmp/out.wav",
            "out_jsonl": "/tmp/out.jsonl",
            "out_manifest": "/tmp/out.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "fixture",
            "sample_rate_hz": 48_000,
        ],
        "checks": [
            [
                "id": ReadinessContractID.modelPath.rawValue,
                "status": "FAIL",
                "detail": "model path missing",
                "remediation": "Provide a compatible model."
            ],
            ["id": ReadinessContractID.screenCaptureAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
            ["id": ReadinessContractID.microphoneAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
        ],
    ]
    return (try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])) ?? Data()
}

private func preflightRuntimeBlockedPayload() -> Data {
    let payload: [String: Any] = [
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "FAIL",
        "config": [
            "out_wav": "/tmp/out.wav",
            "out_jsonl": "/tmp/out.jsonl",
            "out_manifest": "/tmp/out.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "fixture",
            "sample_rate_hz": 48_000,
        ],
        "checks": [
            ["id": ReadinessContractID.modelPath.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
            ["id": ReadinessContractID.outWav.rawValue, "status": "FAIL", "detail": "output path not writable", "remediation": "Choose writable output path."],
            ["id": ReadinessContractID.screenCaptureAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
            ["id": ReadinessContractID.microphoneAccess.rawValue, "status": "PASS", "detail": "ok", "remediation": ""],
        ],
    ]
    return (try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])) ?? Data()
}

@MainActor
private func runSmoke() {
    let store = InMemoryOnboardingCompletionStore()

    let appShell = AppShellViewModel(
        firstRun: nil,
        onboardingCompletionStore: store,
        runtimeReadinessChecker: readyRuntimeReadinessChecker()
    )
    check(appShell.activeRoot == .onboarding, "fresh launch should route to onboarding")
    check(!appShell.isOnboardingComplete, "fresh launch should not be completed")

    let validModel = ModelSetupViewModel(
        modelResolutionService: StubModelResolutionService(
            result: .success(
                ResolvedModelDTO(
                    resolvedPath: URL(fileURLWithPath: "/tmp/model.bin"),
                    source: "fixture",
                    checksumSHA256: nil,
                    checksumStatus: "available"
                )
            )
        )
    )
    validModel.chooseBackend("whispercpp")

    let preflight = PreflightViewModel(
        runner: RecorditPreflightRunner(
            executable: "/usr/bin/env",
            commandRunner: StubCommandRunner(payload: preflightPassPayload()),
            parser: PreflightEnvelopeParser(),
            environment: [:]
        ),
        gatingPolicy: PreflightGatingPolicy()
    )
    preflight.runLivePreflight()

    check(
        appShell.completeOnboardingIfReady(modelSetup: validModel, preflight: preflight),
        "completion should succeed when model and preflight are ready"
    )
    check(appShell.activeRoot == .mainRuntime, "successful completion should route to main runtime")
    check(appShell.isOnboardingComplete, "completion should persist onboarding state")

    let relaunch = AppShellViewModel(
        firstRun: nil,
        onboardingCompletionStore: store,
        runtimeReadinessChecker: readyRuntimeReadinessChecker()
    )
    check(relaunch.activeRoot == .mainRuntime, "relaunch should restore completion and skip onboarding")

    relaunch.resetOnboardingCompletion()
    check(relaunch.activeRoot == .onboarding, "reset should route back to onboarding")
    check(!relaunch.isOnboardingComplete, "reset should clear persisted completion")

    let invalidModel = ModelSetupViewModel(
        modelResolutionService: StubModelResolutionService(
            result: .failure(
                AppServiceError(
                    code: .modelUnavailable,
                    userMessage: "model invalid",
                    remediation: "fix path"
                )
            )
        )
    )
    invalidModel.chooseBackend("whispercpp")
    check(
        !relaunch.completeOnboardingIfReady(modelSetup: invalidModel, preflight: preflight),
        "completion should fail when model setup is invalid"
    )
    check(relaunch.onboardingGateFailure?.code == .modelUnavailable, "model failure should map to modelUnavailable")

    let preflightNotRun = PreflightViewModel(
        runner: RecorditPreflightRunner(
            executable: "/usr/bin/env",
            commandRunner: StubCommandRunner(payload: preflightPassPayload()),
            parser: PreflightEnvelopeParser(),
            environment: [:]
        ),
        gatingPolicy: PreflightGatingPolicy()
    )
    check(
        !relaunch.completeOnboardingIfReady(modelSetup: validModel, preflight: preflightNotRun),
        "completion should fail when preflight has not produced a passable evaluation"
    )
    check(relaunch.onboardingGateFailure?.code == .preflightFailed, "preflight failure should map to preflightFailed")

    let backendBlockedPreflight = PreflightViewModel(
        runner: RecorditPreflightRunner(
            executable: "/usr/bin/env",
            commandRunner: StubCommandRunner(payload: preflightModelBlockedPayload()),
            parser: PreflightEnvelopeParser(),
            environment: [:]
        ),
        gatingPolicy: PreflightGatingPolicy()
    )
    backendBlockedPreflight.runLivePreflight()
    check(
        !relaunch.completeOnboardingIfReady(modelSetup: validModel, preflight: backendBlockedPreflight),
        "completion should fail when backend/model readiness blocks live preflight"
    )
    check(
        relaunch.onboardingGateFailure?.code == .modelUnavailable,
        "backend/model readiness blockers should map to modelUnavailable"
    )
    check(
        relaunch.onboardingGateFailure?.remediation.contains("Record Only remains available") == true,
        "backend/model readiness blockers should surface Record Only fallback guidance"
    )

    let runtimeBlockedPreflight = PreflightViewModel(
        runner: RecorditPreflightRunner(
            executable: "/usr/bin/env",
            commandRunner: StubCommandRunner(payload: preflightRuntimeBlockedPayload()),
            parser: PreflightEnvelopeParser(),
            environment: [:]
        ),
        gatingPolicy: PreflightGatingPolicy()
    )
    runtimeBlockedPreflight.runLivePreflight()
    check(
        !relaunch.completeOnboardingIfReady(modelSetup: validModel, preflight: runtimeBlockedPreflight),
        "completion should fail when runtime preflight readiness blocks live preflight"
    )
    check(
        relaunch.onboardingGateFailure?.code == .preflightFailed,
        "runtime preflight blockers should map to preflightFailed"
    )
    check(
        relaunch.onboardingGateFailure?.remediation.contains("Record Only remains available") == false,
        "runtime preflight blockers should not advertise Record Only fallback"
    )
}

@main
struct OnboardingCompletionSmokeMain {
    @MainActor
    static func main() {
        runSmoke()
        print("onboarding_completion_smoke: PASS")
    }
}
