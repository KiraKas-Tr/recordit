import Foundation

public enum PreflightStatus: String, Codable, Sendable {
    case pass = "PASS"
    case warn = "WARN"
    case fail = "FAIL"
}

public struct PreflightCheckDTO: Codable, Equatable, Sendable {
    public var id: String
    public var status: PreflightStatus
    public var detail: String
    public var remediation: String?
}

public struct PreflightConfigDTO: Codable, Equatable, Sendable {
    public var outWav: String
    public var outJsonl: String
    public var outManifest: String
    public var asrBackend: String
    public var asrModelRequested: String
    public var asrModelResolved: String
    public var asrModelSource: String
    public var sampleRateHz: UInt64

    enum CodingKeys: String, CodingKey {
        case outWav = "out_wav"
        case outJsonl = "out_jsonl"
        case outManifest = "out_manifest"
        case asrBackend = "asr_backend"
        case asrModelRequested = "asr_model_requested"
        case asrModelResolved = "asr_model_resolved"
        case asrModelSource = "asr_model_source"
        case sampleRateHz = "sample_rate_hz"
    }
}

public struct PreflightManifestEnvelopeDTO: Codable, Equatable, Sendable {
    public var schemaVersion: String
    public var kind: String
    public var generatedAtUTC: String
    public var overallStatus: PreflightStatus
    public var config: PreflightConfigDTO
    public var checks: [PreflightCheckDTO]

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case kind
        case generatedAtUTC = "generated_at_utc"
        case overallStatus = "overall_status"
        case config
        case checks
    }
}

public protocol CommandRunning {
    func run(
        executable: String,
        arguments: [String],
        environment: [String: String]
    ) throws -> CommandExecutionResult
}

public struct CommandExecutionResult: Equatable, Sendable {
    public var exitCode: Int32
    public var stdout: Data
    public var stderr: Data
}

public struct ProcessCommandRunner: CommandRunning {
    public init() {}

    public func run(
        executable: String,
        arguments: [String],
        environment: [String: String]
    ) throws -> CommandExecutionResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments

        var mergedEnvironment = ProcessInfo.processInfo.environment
        for (key, value) in environment {
            mergedEnvironment[key] = value
        }
        process.environment = mergedEnvironment

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        do {
            try process.run()
        } catch {
            throw AppServiceError(
                code: .processLaunchFailed,
                userMessage: "Could not launch preflight diagnostics.",
                remediation: "Verify that `recordit` is installed and executable.",
                debugDetail: String(describing: error)
            )
        }

        process.waitUntilExit()
        return CommandExecutionResult(
            exitCode: process.terminationStatus,
            stdout: stdoutPipe.fileHandleForReading.readDataToEndOfFile(),
            stderr: stderrPipe.fileHandleForReading.readDataToEndOfFile()
        )
    }
}

public struct PreflightEnvelopeParser {
    public static let expectedKind = "transcribe-live-preflight"
    public static let expectedSchemaVersion = "1"

    public init() {}

    public func parse(data: Data) throws -> PreflightManifestEnvelopeDTO {
        let decoder = JSONDecoder()
        let envelope: PreflightManifestEnvelopeDTO
        do {
            envelope = try decoder.decode(PreflightManifestEnvelopeDTO.self, from: data)
        } catch {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Preflight output is malformed.",
                remediation: "Re-run preflight and verify JSON output contract compatibility.",
                debugDetail: String(describing: error)
            )
        }

        guard envelope.kind == Self.expectedKind else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Preflight output kind is not supported.",
                remediation: "Update the app shell parser to a compatible preflight contract.",
                debugDetail: "kind=\(envelope.kind)"
            )
        }
        guard envelope.schemaVersion == Self.expectedSchemaVersion else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Preflight schema version is not supported.",
                remediation: "Update parser compatibility for the manifest schema version in use.",
                debugDetail: "schema_version=\(envelope.schemaVersion)"
            )
        }
        return envelope
    }
}

public struct RecorditPreflightRunner {
    public static let deterministicArguments = ["preflight", "--mode", "live", "--json"]

    private let executable: String
    private let commandRunner: CommandRunning
    private let parser: PreflightEnvelopeParser
    private let environment: [String: String]

    public init(
        executable: String = "/usr/bin/env",
        commandRunner: CommandRunning = ProcessCommandRunner(),
        parser: PreflightEnvelopeParser = PreflightEnvelopeParser(),
        environment: [String: String] = [:]
    ) {
        self.executable = executable
        self.commandRunner = commandRunner
        self.parser = parser
        self.environment = environment
    }

    public func runLivePreflight() throws -> PreflightManifestEnvelopeDTO {
        let invocation: [String]
        if executable == "/usr/bin/env" {
            invocation = ["recordit"] + Self.deterministicArguments
        } else {
            invocation = Self.deterministicArguments
        }
        let result = try commandRunner.run(
            executable: executable,
            arguments: invocation,
            environment: environment
        )
        guard result.exitCode == 0 else {
            throw AppServiceError(
                code: .preflightFailed,
                userMessage: "Preflight checks failed.",
                remediation: "Review check statuses and complete the recommended remediation steps.",
                debugDetail: String(data: result.stderr, encoding: .utf8)
            )
        }
        guard !result.stdout.isEmpty else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Preflight produced no JSON output.",
                remediation: "Run preflight again and ensure `--json` output is enabled."
            )
        }
        return try parser.parse(data: result.stdout)
    }
}
