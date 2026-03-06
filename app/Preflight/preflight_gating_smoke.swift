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

private func fixtureEnvelope(
    checks: [PreflightCheckDTO],
    overallStatus: PreflightStatus = .warn
) -> PreflightManifestEnvelopeDTO {
    PreflightManifestEnvelopeDTO(
        schemaVersion: "1",
        kind: "transcribe-live-preflight",
        generatedAtUTC: "2026-03-05T00:00:00Z",
        overallStatus: overallStatus,
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
    let expectedKnown: Set<String> = ReadinessContract.knownContractIDs

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
    for id in ReadinessContract.tccCaptureIDs {
        check(
            ReadinessContract.domain(forCheckID: id) == .tccCapture,
            "tcc check \(id) must map to tcc_capture domain"
        )
    }
    for id in ReadinessContract.backendModelIDs {
        check(
            ReadinessContract.domain(forCheckID: id) == .backendModel,
            "backend model check \(id) must map to backend_model domain"
        )
    }
    for id in ReadinessContract.runtimePreflightIDs {
        check(
            ReadinessContract.domain(forCheckID: id) == .runtimePreflight,
            "runtime preflight check \(id) must map to runtime_preflight domain"
        )
    }
    for id in ReadinessContract.backendRuntimeIDs {
        check(
            ReadinessContract.domain(forCheckID: id) == .backendRuntime,
            "backend runtime check \(id) must map to backend_runtime domain"
        )
    }

    let policy = PreflightGatingPolicy()

    let blockedEnvelope = fixtureEnvelope(checks: [
        fixtureCheck(ReadinessContractID.modelPath.rawValue, .fail),
        fixtureCheck(ReadinessContractID.backendRuntime.rawValue, .warn),
    ])
    let blocked = policy.evaluate(blockedEnvelope)
    check(blocked.blockingFailures.count == 1, "model_path fail must block")
    check(
        blocked.blockingFailures[0].check.id == ReadinessContractID.modelPath.rawValue,
        "unexpected blocking ID"
    )
    check(blocked.primaryBlockingDomain == .backendModel, "model_path fail should map to backend_model domain")
    check(blocked.recordOnlyFallbackEligible, "backend/model blockers should keep Record Only fallback eligible")
    check(!blocked.canProceed(acknowledgingWarnings: false), "blocking fail must prevent proceed")
    check(!blocked.canProceed(acknowledgingWarnings: true), "blocking fail must prevent proceed even with warning ack")

    let warnOnlyEnvelope = fixtureEnvelope(checks: [
        fixtureCheck(ReadinessContractID.modelPath.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outWav.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outJsonl.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outManifest.rawValue, .pass),
        fixtureCheck(ReadinessContractID.screenCaptureAccess.rawValue, .pass),
        fixtureCheck(ReadinessContractID.microphoneAccess.rawValue, .pass),
        fixtureCheck(ReadinessContractID.sampleRate.rawValue, .warn),
        fixtureCheck(ReadinessContractID.backendRuntime.rawValue, .warn),
    ])
    let warnOnly = policy.evaluate(warnOnlyEnvelope)
    check(warnOnly.blockingFailures.isEmpty, "warn-only envelope should not have blockers")
    check(warnOnly.warningContinuations.count == 2, "warn-only envelope should require warning ack")
    check(!warnOnly.recordOnlyFallbackEligible, "warn-only envelope should not be treated as fallback-eligible")
    check(!warnOnly.canProceed(acknowledgingWarnings: false), "warn-only envelope must require explicit acknowledgment")
    check(warnOnly.canProceed(acknowledgingWarnings: true), "warn-only envelope should proceed after acknowledgment")

    let runtimePreflightBlockedEnvelope = fixtureEnvelope(checks: [
        fixtureCheck(ReadinessContractID.modelPath.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outWav.rawValue, .fail),
        fixtureCheck(ReadinessContractID.screenCaptureAccess.rawValue, .pass),
        fixtureCheck(ReadinessContractID.microphoneAccess.rawValue, .pass),
    ])
    let runtimePreflightBlocked = policy.evaluate(runtimePreflightBlockedEnvelope)
    check(
        runtimePreflightBlocked.primaryBlockingDomain == .runtimePreflight,
        "out_wav fail should map to runtime_preflight domain"
    )
    check(
        !runtimePreflightBlocked.recordOnlyFallbackEligible,
        "runtime preflight blockers should not automatically mark Record Only fallback eligible"
    )

    let diagnosticOnlyEnvelope = fixtureEnvelope(checks: [
        fixtureCheck(ReadinessContractID.modelReadability.rawValue, .fail),
    ])
    let diagnosticOnly = policy.evaluate(diagnosticOnlyEnvelope)
    check(
        diagnosticOnly.mappedChecks.first?.isKnownContractID == true,
        "diagnostic-only IDs should be tracked as known contract IDs"
    )
    check(
        diagnosticOnly.unknownCheckIDs.isEmpty,
        "diagnostic-only IDs should not be reported as unknown"
    )
}

@MainActor
private func runViewModelChecks() {
    let warnEnvelope = fixtureEnvelope(checks: [
        fixtureCheck(ReadinessContractID.modelPath.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outWav.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outJsonl.rawValue, .pass),
        fixtureCheck(ReadinessContractID.outManifest.rawValue, .pass),
        fixtureCheck(ReadinessContractID.displayAvailability.rawValue, .pass),
        fixtureCheck(ReadinessContractID.microphoneAccess.rawValue, .pass),
        fixtureCheck(ReadinessContractID.sampleRate.rawValue, .warn),
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

    let runtimePermissionFailureEnvelope = fixtureEnvelope(
        checks: [
            fixtureCheck(ReadinessContractID.modelPath.rawValue, .pass),
            fixtureCheck(ReadinessContractID.outWav.rawValue, .pass),
            fixtureCheck(ReadinessContractID.outJsonl.rawValue, .pass),
            fixtureCheck(ReadinessContractID.outManifest.rawValue, .pass),
            fixtureCheck(ReadinessContractID.screenCaptureAccess.rawValue, .fail),
            fixtureCheck(ReadinessContractID.microphoneAccess.rawValue, .fail),
        ],
        overallStatus: .fail
    )
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
    check(
        fallbackViewModel.primaryBlockingDomain == .tccCapture,
        "runtime permission failures should map to tcc_capture domain"
    )
    check(
        !fallbackViewModel.canOfferRecordOnlyFallback,
        "permission blockers should keep Record Only fallback disabled"
    )
    if case let .completed(envelope) = fallbackViewModel.state {
        check(envelope.overallStatus == .fail, "runtime permission failures should keep overall status fail")
        let remainingPermissionFailures = envelope.checks.filter {
            (ReadinessContract.screenPermissionIDs.contains($0.id)
                || $0.id == ReadinessContract.microphonePermissionID)
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
