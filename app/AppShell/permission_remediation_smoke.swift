import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("permission_remediation_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func fixtureEnvelope(checks: [[String: Any]]) -> [String: Any] {
    [
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
            "asr_model_source": "cli",
            "sample_rate_hz": 48_000,
        ],
        "checks": checks,
    ]
}

private func encodeEnvelopeJSON(_ object: [String: Any]) -> Data {
    do {
        return try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    } catch {
        fputs("permission_remediation_smoke failed: could not encode fixture JSON: \(error)\n", stderr)
        exit(1)
    }
}

private final class SequenceCommandRunner: CommandRunning {
    private var payloads: [Data]
    private(set) var invocationCount: Int = 0

    init(payloads: [Data]) {
        self.payloads = payloads
    }

    func run(
        executable _: String,
        arguments _: [String],
        environment _: [String: String]
    ) throws -> CommandExecutionResult {
        let index = min(invocationCount, payloads.count - 1)
        invocationCount += 1
        return CommandExecutionResult(exitCode: 0, stdout: payloads[index], stderr: Data())
    }
}

@MainActor
private func runSmoke() {
    let failingPayload = encodeEnvelopeJSON(
        fixtureEnvelope(checks: [
            [
                "id": "screen_capture_access",
                "status": "FAIL",
                "detail": "screen capture not authorized",
                "remediation": "Grant Screen Recording in System Settings.",
            ],
            [
                "id": "microphone_access",
                "status": "PASS",
                "detail": "microphone sample observed",
                "remediation": "",
            ],
        ])
    )
    let passingPayload = encodeEnvelopeJSON(
        fixtureEnvelope(checks: [
            [
                "id": "screen_capture_access",
                "status": "PASS",
                "detail": "screen access granted",
                "remediation": "",
            ],
            [
                "id": "microphone_access",
                "status": "PASS",
                "detail": "microphone sample observed",
                "remediation": "",
            ],
        ])
    )

    let commandRunner = SequenceCommandRunner(payloads: [failingPayload, passingPayload])
    let runner = RecorditPreflightRunner(
        executable: "/usr/bin/env",
        commandRunner: commandRunner,
        parser: PreflightEnvelopeParser(),
        environment: [:]
    )

    var openedURLs = [URL]()
    let viewModel = PermissionRemediationViewModel(
        runner: runner,
        openSystemSettings: { openedURLs.append($0) }
    )

    viewModel.runPermissionCheck()
    guard case let .ready(items) = viewModel.state else {
        check(false, "initial run should produce ready state")
        return
    }
    let screen = items.first { $0.permission == .screenRecording }
    let mic = items.first { $0.permission == .microphone }
    check(screen?.status == .missing, "screen permission should be missing")
    check(mic?.status == .granted, "microphone permission should be granted")
    check(viewModel.missingPermissions == [.screenRecording], "missing permission set should include screen only")

    let openedScreen = viewModel.openSettings(for: .screenRecording)
    check(openedScreen, "open settings should succeed for screen")
    check(viewModel.shouldShowScreenRecordingRestartAdvisory, "screen settings action should show restart advisory")
    check(
        openedURLs.last?.absoluteString.contains("Privacy_ScreenCapture") == true,
        "screen settings URL should target ScreenCapture privacy pane"
    )

    viewModel.recheckPermissions()
    guard case let .ready(recheckedItems) = viewModel.state else {
        check(false, "re-check should produce ready state")
        return
    }
    let recheckedScreen = recheckedItems.first { $0.permission == .screenRecording }
    check(recheckedScreen?.status == .granted, "screen permission should be granted after re-check payload")
    check(viewModel.missingPermissions.isEmpty, "missing permissions should be empty after pass")

    let openedMic = viewModel.openSettings(for: .microphone)
    check(openedMic, "open settings should succeed for microphone")
    check(
        openedURLs.last?.absoluteString.contains("Privacy_Microphone") == true,
        "microphone settings URL should target Microphone privacy pane"
    )
    check(commandRunner.invocationCount == 2, "runner should be invoked once for initial check and once for re-check")
}

@main
struct PermissionRemediationSmokeMain {
    static func main() async {
        await MainActor.run {
            runSmoke()
        }
        print("permission_remediation_smoke: PASS")
    }
}
