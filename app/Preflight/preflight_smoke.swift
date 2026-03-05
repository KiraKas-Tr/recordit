import Foundation

private final class StubCommandRunner: CommandRunning {
    private let result: CommandExecutionResult
    private(set) var receivedExecutable: String?
    private(set) var receivedArguments: [String] = []
    private(set) var receivedEnvironment: [String: String] = [:]

    init(result: CommandExecutionResult) {
        self.result = result
    }

    func run(
        executable: String,
        arguments: [String],
        environment: [String: String]
    ) throws -> CommandExecutionResult {
        receivedExecutable = executable
        receivedArguments = arguments
        receivedEnvironment = environment
        return result
    }
}

@MainActor
private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("preflight_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func encodedData(_ payload: [String: Any]) -> Data {
    do {
        return try JSONSerialization.data(withJSONObject: payload, options: [])
    } catch {
        fputs("preflight_smoke failed: could not encode fixture JSON: \(error)\n", stderr)
        exit(1)
    }
}

private func validEnvelopeData() -> Data {
    encodedData([
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "PASS",
        "config": [
            "out_wav": "/tmp/session.wav",
            "out_jsonl": "/tmp/session.jsonl",
            "out_manifest": "/tmp/session.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "cli",
            "sample_rate_hz": 48000
        ],
        "checks": [
            [
                "id": "permissions",
                "status": "PASS",
                "detail": "all permissions present",
                "remediation": NSNull()
            ],
            [
                "id": "model",
                "status": "WARN",
                "detail": "model checksum unavailable",
                "remediation": "run model doctor"
            ]
        ]
    ])
}

@MainActor
private func runSmoke() {
    let parser = PreflightEnvelopeParser()

    let valid: PreflightManifestEnvelopeDTO
    do {
        valid = try parser.parse(data: validEnvelopeData())
    } catch {
        check(false, "valid envelope should parse: \(error)")
        return
    }
    check(valid.kind == "transcribe-live-preflight", "expected valid preflight kind")
    check(valid.schemaVersion == "1", "expected schema_version=1")
    check(valid.checks.count == 2, "expected two decoded checks")

    let wrongKindData = encodedData([
        "schema_version": "1",
        "kind": "transcribe-live-runtime",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "PASS",
        "config": [
            "out_wav": "/tmp/session.wav",
            "out_jsonl": "/tmp/session.jsonl",
            "out_manifest": "/tmp/session.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "cli",
            "sample_rate_hz": 48000
        ],
        "checks": []
    ])
    do {
        _ = try parser.parse(data: wrongKindData)
        check(false, "wrong kind should fail envelope validation")
    } catch let error as AppServiceError {
        check(error.code == .manifestInvalid, "wrong kind should map to manifestInvalid")
    } catch {
        check(false, "wrong kind emitted unexpected error type")
    }

    let malformedChecksData = encodedData([
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "overall_status": "PASS",
        "config": [
            "out_wav": "/tmp/session.wav",
            "out_jsonl": "/tmp/session.jsonl",
            "out_manifest": "/tmp/session.manifest.json",
            "asr_backend": "whispercpp",
            "asr_model_requested": "/tmp/model.bin",
            "asr_model_resolved": "/tmp/model.bin",
            "asr_model_source": "cli",
            "sample_rate_hz": 48000
        ],
        "checks": [
            [
                "id": "permissions",
                "status": 42,
                "detail": "wrong type",
                "remediation": NSNull()
            ]
        ]
    ])
    do {
        _ = try parser.parse(data: malformedChecksData)
        check(false, "malformed checks should fail decoding")
    } catch let error as AppServiceError {
        check(error.code == .manifestInvalid, "malformed checks should map to manifestInvalid")
    } catch {
        check(false, "malformed checks emitted unexpected error type")
    }

    let stub = StubCommandRunner(
        result: CommandExecutionResult(
            exitCode: 0,
            stdout: validEnvelopeData(),
            stderr: Data()
        )
    )
    let runner = RecorditPreflightRunner(
        executable: "/usr/bin/env",
        commandRunner: stub,
        parser: parser,
        environment: ["RECORDIT_TEST": "1"]
    )
    let envelope: PreflightManifestEnvelopeDTO
    do {
        envelope = try runner.runLivePreflight()
    } catch {
        check(false, "runner should parse valid envelope: \(error)")
        return
    }
    check(envelope.kind == "transcribe-live-preflight", "runner should return parsed envelope")
    check(stub.receivedExecutable == "/usr/bin/env", "runner should use configured executable")
    check(
        stub.receivedArguments == ["recordit", "preflight", "--mode", "live", "--json"],
        "runner should use deterministic recordit preflight args"
    )
    check(stub.receivedEnvironment["RECORDIT_TEST"] == "1", "runner should pass through environment")
}

@main
struct PreflightSmokeMain {
    @MainActor
    static func main() {
        runSmoke()
        print("preflight_smoke: PASS")
    }
}
