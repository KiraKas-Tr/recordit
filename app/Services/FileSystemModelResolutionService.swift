import CryptoKit
import Foundation

public struct FileSystemModelResolutionService: ModelResolutionService {
    private let environment: [String: String]
    private let currentDirectoryURL: URL

    public init(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        currentDirectoryURL: URL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    ) {
        self.environment = environment
        self.currentDirectoryURL = currentDirectoryURL.standardizedFileURL
    }

    public func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        let backend = try parseBackend(request.backend)
        let candidate = try resolveCandidate(explicitPath: request.explicitModelPath, backend: backend)
        let path = candidate.path

        guard FileManager.default.fileExists(atPath: path.path) else {
            throw AppServiceError(
                code: .modelUnavailable,
                userMessage: "Model path was not found.",
                remediation: "Choose an existing local model path for the selected backend.",
                debugDetail: path.path
            )
        }

        try validateBackendKind(backend: backend, path: path)
        let checksum = checksumInfo(path: path)

        return ResolvedModelDTO(
            resolvedPath: path,
            source: candidate.source,
            checksumSHA256: checksum.sha256,
            checksumStatus: checksum.status
        )
    }

    private func parseBackend(_ raw: String) throws -> BackendKind {
        guard let backend = BackendKind(rawValue: raw.lowercased()) else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Model backend is not supported.",
                remediation: "Use `whispercpp` or `whisperkit` in v1 model setup.",
                debugDetail: "backend=\(raw)"
            )
        }
        return backend
    }

    private func resolveCandidate(
        explicitPath: URL?,
        backend: BackendKind
    ) throws -> (path: URL, source: String) {
        if let explicitPath {
            let path = absolutize(explicitPath)
            return (path, "ui selected path")
        }

        if let envValue = environment["RECORDIT_ASR_MODEL"]?.trimmingCharacters(in: .whitespacesAndNewlines),
           !envValue.isEmpty {
            return (absolutize(URL(fileURLWithPath: envValue)), "RECORDIT_ASR_MODEL")
        }

        for defaultPath in defaultCandidates(for: backend) {
            let absolute = absolutize(defaultPath)
            if FileManager.default.fileExists(atPath: absolute.path) {
                return (absolute, "backend default")
            }
        }

        throw AppServiceError(
            code: .modelUnavailable,
            userMessage: "No compatible local model was found.",
            remediation: "Choose an existing model path or set RECORDIT_ASR_MODEL.",
            debugDetail: "backend=\(backend.rawValue)"
        )
    }

    private func validateBackendKind(backend: BackendKind, path: URL) throws {
        var isDirectory: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: path.path, isDirectory: &isDirectory)
        guard exists else {
            throw AppServiceError(
                code: .modelUnavailable,
                userMessage: "Model path was not found.",
                remediation: "Choose an existing local model path for the selected backend.",
                debugDetail: path.path
            )
        }

        switch backend {
        case .whispercpp where isDirectory.boolValue:
            throw AppServiceError(
                code: .modelUnavailable,
                userMessage: "Selected model path is incompatible with whispercpp.",
                remediation: "Choose a model file path for whispercpp.",
                debugDetail: path.path
            )
        case .whisperkit where !isDirectory.boolValue:
            throw AppServiceError(
                code: .modelUnavailable,
                userMessage: "Selected model path is incompatible with whisperkit.",
                remediation: "Choose a model directory path for whisperkit.",
                debugDetail: path.path
            )
        default:
            break
        }
    }

    private func checksumInfo(path: URL) -> (sha256: String?, status: String) {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path.path, isDirectory: &isDirectory) else {
            return (nil, "unavailable_unresolved")
        }
        if isDirectory.boolValue {
            return (nil, "unavailable_directory")
        }
        guard FileManager.default.isReadableFile(atPath: path.path) else {
            return (nil, "unavailable_not_file")
        }

        do {
            let digest = try sha256Hex(path: path)
            return (digest, "available")
        } catch {
            return (nil, "unavailable_checksum_error")
        }
    }

    private func sha256Hex(path: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: path)
        defer { try? handle.close() }

        var hasher = SHA256()
        while true {
            let data = try handle.read(upToCount: 1_048_576) ?? Data()
            if data.isEmpty {
                break
            }
            hasher.update(data: data)
        }

        let digest = hasher.finalize()
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    private func absolutize(_ path: URL) -> URL {
        let standardized: URL
        if path.isFileURL {
            standardized = path.standardizedFileURL
        } else {
            standardized = URL(fileURLWithPath: path.path).standardizedFileURL
        }
        if standardized.path.hasPrefix("/") {
            return standardized
        }
        return currentDirectoryURL
            .appendingPathComponent(standardized.path)
            .standardizedFileURL
    }

    private func defaultCandidates(for backend: BackendKind) -> [URL] {
        switch backend {
        case .whispercpp:
            return [
                URL(fileURLWithPath: "artifacts/bench/models/whispercpp/ggml-tiny.en.bin"),
                URL(fileURLWithPath: "models/ggml-tiny.en.bin"),
            ]
        case .whisperkit:
            return [
                URL(
                    fileURLWithPath:
                        "artifacts/bench/models/whisperkit/models/argmaxinc/whisperkit-coreml/openai_whisper-tiny"
                ),
                URL(fileURLWithPath: "models/whisperkit/openai_whisper-tiny"),
            ]
        }
    }
}

private enum BackendKind: String {
    case whispercpp
    case whisperkit
}
