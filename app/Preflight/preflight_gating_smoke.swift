import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("preflight_gating_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func fixtureCheck(_ id: String, _ status: PreflightStatus) -> PreflightCheckDTO {
    PreflightCheckDTO(
        id: id,
        status: status,
        detail: "\(id) detail",
        remediation: "\(id) remediation"
    )
}

private func fixtureEnvelope(checks: [PreflightCheckDTO]) -> PreflightManifestEnvelopeDTO {
    PreflightManifestEnvelopeDTO(
        schemaVersion: "1",
        kind: "transcribe-live-preflight",
        generatedAtUTC: "2026-03-05T00:00:00Z",
        overallStatus: .warn,
        config: PreflightConfigDTO(
            outWav: "/tmp/out.wav",
            outJsonl: "/tmp/out.jsonl",
            outManifest: "/tmp/out.manifest.json",
            asrBackend: "whispercpp",
            asrModelRequested: "/tmp/model.bin",
            asrModelResolved: "/tmp/model.bin",
            asrModelSource: "cli",
            sampleRateHz: 48_000
        ),
        checks: checks
    )
}

private struct StubCommandRunner: CommandRunning {
    let stdoutData: Data

    func run(
        executable _: String,
        arguments _: [String],
        environment _: [String: String]
    ) throws -> CommandExecutionResult {
        CommandExecutionResult(exitCode: 0, stdout: stdoutData, stderr: Data())
    }
}

private func runPolicyChecks() {
    let expectedKnown: Set<String> = [
        "model_path",
        "out_wav",
        "out_jsonl",
        "out_manifest",
        "screen_capture_access",
        "display_availability",
        "microphone_access",
        "sample_rate",
        "backend_runtime",
    ]

    check(
        PreflightGatingPolicy.knownContractCheckIDs == expectedKnown,
        "known contract check ID mapping drifted"
    )

    for id in PreflightGatingPolicy.blockingFailureCheckIDs {
        check(
            PreflightGatingPolicy.policy(forCheckID: id) == .blockOnFail,
            "blocking check \(id) must map to blockOnFail"
        )
    }
    for id in PreflightGatingPolicy.warnAcknowledgementCheckIDs {
        check(
            PreflightGatingPolicy.policy(forCheckID: id) == .warnRequiresAcknowledgement,
            "warn check \(id) must map to warnRequiresAcknowledgement"
        )
    }

    let policy = PreflightGatingPolicy()

    let blockedEnvelope = fixtureEnvelope(checks: [
        fixtureCheck("model_path", .fail),
        fixtureCheck("backend_runtime", .warn),
    ])
    let blocked = policy.evaluate(blockedEnvelope)
    check(blocked.blockingFailures.count == 1, "model_path fail must block")
    check(blocked.blockingFailures[0].check.id == "model_path", "unexpected blocking ID")
    check(!blocked.canProceed(acknowledgingWarnings: false), "blocking fail must prevent proceed")
    check(!blocked.canProceed(acknowledgingWarnings: true), "blocking fail must prevent proceed even with warning ack")

    let warnOnlyEnvelope = fixtureEnvelope(checks: [
        fixtureCheck("model_path", .pass),
        fixtureCheck("out_wav", .pass),
        fixtureCheck("out_jsonl", .pass),
        fixtureCheck("out_manifest", .pass),
        fixtureCheck("screen_capture_access", .pass),
        fixtureCheck("microphone_access", .pass),
        fixtureCheck("sample_rate", .warn),
        fixtureCheck("backend_runtime", .warn),
    ])
    let warnOnly = policy.evaluate(warnOnlyEnvelope)
    check(warnOnly.blockingFailures.isEmpty, "warn-only envelope should not have blockers")
    check(warnOnly.warningContinuations.count == 2, "warn-only envelope should require warning ack")
    check(!warnOnly.canProceed(acknowledgingWarnings: false), "warn-only envelope must require explicit acknowledgment")
    check(warnOnly.canProceed(acknowledgingWarnings: true), "warn-only envelope should proceed after acknowledgment")
}

@MainActor
private func runViewModelChecks() {
    let warnEnvelope = fixtureEnvelope(checks: [
        fixtureCheck("model_path", .pass),
        fixtureCheck("out_wav", .pass),
        fixtureCheck("out_jsonl", .pass),
        fixtureCheck("out_manifest", .pass),
        fixtureCheck("display_availability", .pass),
        fixtureCheck("microphone_access", .pass),
        fixtureCheck("sample_rate", .warn),
    ])
    let encoder = JSONEncoder()
    let data: Data
    do {
        data = try encoder.encode(warnEnvelope)
    } catch {
        fputs("preflight_gating_smoke failed: fixture encode failed: \(error)\n", stderr)
        exit(1)
    }

    let runner = RecorditPreflightRunner(
        executable: "/usr/bin/env",
        commandRunner: StubCommandRunner(stdoutData: data),
        parser: PreflightEnvelopeParser(),
        environment: [:]
    )
    let viewModel = PreflightViewModel(runner: runner, gatingPolicy: PreflightGatingPolicy())
    viewModel.runLivePreflight()
    check(viewModel.requiresWarningAcknowledgement, "view model should require warning acknowledgment")
    check(!viewModel.canProceedToLiveTranscribe, "view model must block proceed until user acknowledges warnings")
    viewModel.acknowledgeWarningsForLiveTranscribe()
    check(viewModel.canProceedToLiveTranscribe, "view model should allow proceed after user acknowledgment")

    let runtimePermissionFailureEnvelope = fixtureEnvelope(checks: [
        fixtureCheck("model_path", .pass),
        fixtureCheck("out_wav", .pass),
        fixtureCheck("out_jsonl", .pass),
        fixtureCheck("out_manifest", .pass),
        fixtureCheck("screen_capture_access", .fail),
        fixtureCheck("microphone_access", .fail),
    ])
    let runtimeFailureData: Data
    do {
        runtimeFailureData = try encoder.encode(runtimePermissionFailureEnvelope)
    } catch {
        fputs("preflight_gating_smoke failed: runtime failure fixture encode failed: \(error)\n", stderr)
        exit(1)
    }

    let runtimeFailureRunner = RecorditPreflightRunner(
        executable: "/usr/bin/env",
        commandRunner: StubCommandRunner(stdoutData: runtimeFailureData),
        parser: PreflightEnvelopeParser(),
        environment: [:]
    )
    let fallbackViewModel = PreflightViewModel(
        runner: runtimeFailureRunner,
        gatingPolicy: PreflightGatingPolicy(),
        nativePermissionStatus: { _ in true }
    )
    fallbackViewModel.runLivePreflight()
    check(
        !fallbackViewModel.canProceedToLiveTranscribe,
        "runtime permission failures must block proceed in production mode"
    )
    if case let .completed(envelope) = fallbackViewModel.state {
        check(envelope.overallStatus == .fail, "runtime permission failures should keep overall status fail")
        let remainingPermissionFailures = envelope.checks.filter {
            ($0.id == "screen_capture_access" || $0.id == "display_availability" || $0.id == "microphone_access")
                && $0.status == .fail
        }
        check(
            !remainingPermissionFailures.isEmpty,
            "runtime permission checks should remain in fail state until helper probes succeed"
        )
    } else {
        check(false, "fallback view model should complete with a runtime envelope")
    }
}

@main
struct PreflightGatingSmokeMain {
    static func main() async {
        runPolicyChecks()
        await MainActor.run {
            runViewModelChecks()
        }
        print("preflight_gating_smoke: PASS")
    }
}
