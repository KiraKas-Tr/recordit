import Foundation

public struct FileSystemPendingSessionSidecarService: PendingSessionSidecarService {
    public typealias DataReader = @Sendable (URL) throws -> Data
    public typealias DataWriter = @Sendable (Data, URL) throws -> Void
    public typealias DirectoryCreator = @Sendable (URL) throws -> Void
    public typealias FileExists = @Sendable (URL) -> Bool
    public typealias ReplaceItem = @Sendable (URL, URL) throws -> Void
    public typealias MoveItem = @Sendable (URL, URL) throws -> Void
    public typealias RemoveItem = @Sendable (URL) throws -> Void

    private let dataReader: DataReader
    private let dataWriter: DataWriter
    private let directoryCreator: DirectoryCreator
    private let fileExists: FileExists
    private let replaceItem: ReplaceItem
    private let moveItem: MoveItem
    private let removeItem: RemoveItem

    public init(
        dataReader: @escaping DataReader = { url in
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            return try handle.readToEnd() ?? Data()
        },
        dataWriter: @escaping DataWriter = { data, destination in try data.write(to: destination) },
        directoryCreator: @escaping DirectoryCreator = {
            try FileManager.default.createDirectory(at: $0, withIntermediateDirectories: true)
        },
        fileExists: @escaping FileExists = { FileManager.default.fileExists(atPath: $0.path) },
        replaceItem: @escaping ReplaceItem = { destination, staging in
            _ = try FileManager.default.replaceItemAt(destination, withItemAt: staging)
        },
        moveItem: @escaping MoveItem = { from, to in
            try FileManager.default.moveItem(at: from, to: to)
        },
        removeItem: @escaping RemoveItem = { try FileManager.default.removeItem(at: $0) }
    ) {
        self.dataReader = dataReader
        self.dataWriter = dataWriter
        self.directoryCreator = directoryCreator
        self.fileExists = fileExists
        self.replaceItem = replaceItem
        self.moveItem = moveItem
        self.removeItem = removeItem
    }

    public func writePendingSidecar(_ request: PendingSessionSidecarWriteRequest) throws -> PendingSessionSidecarDTO {
        let sessionRoot = try absoluteURL(request.sessionRoot, field: "sessionRoot")
        let wavPath = try absoluteURL(request.wavPath, field: "wavPath")
        let sessionID = normalizedSessionID(request.sessionID, fallback: sessionRoot.lastPathComponent)

        guard request.mode == .recordOnly else {
            throw invalidSidecarError(
                detail: "mode must be record_only for pending sidecar writes"
            )
        }

        do {
            try directoryCreator(sessionRoot)
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not prepare the pending session folder.",
                remediation: "Verify output folder permissions and retry.",
                debugDetail: "\(sessionRoot.path): \(error)"
            )
        }

        let sidecar = PendingSessionSidecarDTO(
            sessionID: sessionID,
            createdAtUTC: Self.iso8601FormatterWithFractional.string(from: request.createdAt),
            wavPath: wavPath.path,
            mode: .recordOnly,
            transcriptionState: request.transcriptionState
        )

        let pendingURL = sessionRoot.appendingPathComponent("session.pending.json")
        let stagingURL = sessionRoot.appendingPathComponent(".session.pending.\(UUID().uuidString).tmp")

        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys, .prettyPrinted]
            let data = try encoder.encode(sidecar)
            try dataWriter(data, stagingURL)
            try replaceItemAtomically(stagingItem: stagingURL, destination: pendingURL)
            return sidecar
        } catch let serviceError as AppServiceError {
            try? removeItem(stagingURL)
            throw serviceError
        } catch {
            try? removeItem(stagingURL)
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not write pending session metadata.",
                remediation: "Verify session folder permissions and retry.",
                debugDetail: "\(pendingURL.path): \(error)"
            )
        }
    }

    public func loadPendingSidecar(at pendingSidecarPath: URL) throws -> PendingSessionSidecarDTO {
        let normalizedURL = pendingSidecarPath.standardizedFileURL
        let data: Data
        do {
            data = try dataReader(normalizedURL)
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Pending metadata could not be read.",
                remediation: "Verify session folder readability and retry.",
                debugDetail: "\(normalizedURL.path): \(error)"
            )
        }

        let sidecar: PendingSessionSidecarDTO
        do {
            sidecar = try JSONDecoder().decode(PendingSessionSidecarDTO.self, from: data)
        } catch {
            throw invalidSidecarError(
                detail: "malformed JSON payload at \(normalizedURL.path): \(error)"
            )
        }

        return try validate(sidecar: sidecar, sourceURL: normalizedURL)
    }

    public func initialState(for request: RuntimeStartRequest) -> PendingTranscriptionState {
        request.modelPath == nil ? .pendingModel : .readyToTranscribe
    }

    public func loadAndValidateSidecar(
        at pendingSidecarPath: URL,
        expectedSessionRoot: URL
    ) throws -> PendingSessionSidecarDTO {
        let sidecar = try loadPendingSidecar(at: pendingSidecarPath)
        let expectedWavPath = expectedSessionRoot.standardizedFileURL
            .appendingPathComponent("session.wav")
            .standardizedFileURL
            .path
        guard sidecar.wavPath == expectedWavPath else {
            throw invalidSidecarError(
                detail: "wav_path must equal \(expectedWavPath)"
            )
        }
        return sidecar
    }

    private func validate(
        sidecar: PendingSessionSidecarDTO,
        sourceURL: URL
    ) throws -> PendingSessionSidecarDTO {
        let sessionID = sidecar.sessionID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sessionID.isEmpty else {
            throw invalidSidecarError(detail: "session_id must be non-empty")
        }

        guard parseISO8601(sidecar.createdAtUTC) != nil else {
            throw invalidSidecarError(detail: "created_at_utc must be valid ISO8601")
        }

        let wavURL = URL(fileURLWithPath: sidecar.wavPath).standardizedFileURL
        guard wavURL.path.hasPrefix("/") else {
            throw invalidSidecarError(detail: "wav_path must be absolute")
        }

        guard sidecar.mode == .recordOnly else {
            throw invalidSidecarError(detail: "mode must be record_only")
        }

        return PendingSessionSidecarDTO(
            sessionID: sessionID,
            createdAtUTC: sidecar.createdAtUTC,
            wavPath: wavURL.path,
            mode: .recordOnly,
            transcriptionState: sidecar.transcriptionState
        )
    }

    private func absoluteURL(_ url: URL, field: String) throws -> URL {
        let standardized = url.standardizedFileURL
        guard standardized.path.hasPrefix("/") else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Pending session path is invalid.",
                remediation: "Use absolute paths for pending session metadata.",
                debugDetail: "\(field) must be absolute"
            )
        }
        return standardized
    }

    private func replaceItemAtomically(stagingItem: URL, destination: URL) throws {
        if fileExists(destination) {
            do {
                try replaceItem(destination, stagingItem)
            } catch {
                throw AppServiceError(
                    code: .ioFailure,
                    userMessage: "Could not replace pending metadata atomically.",
                    remediation: "Verify output folder permissions and retry.",
                    debugDetail: "\(destination.path): \(error)"
                )
            }
        } else {
            try moveItem(stagingItem, destination)
        }
    }

    private func normalizedSessionID(_ sessionID: String, fallback: String) -> String {
        let trimmed = sessionID.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? fallback : trimmed
    }

    private func parseISO8601(_ value: String) -> Date? {
        Self.iso8601FormatterWithFractional.date(from: value) ?? Self.iso8601Formatter.date(from: value)
    }

    private func invalidSidecarError(detail: String) -> AppServiceError {
        AppServiceError(
            code: .manifestInvalid,
            userMessage: "Pending metadata is invalid.",
            remediation: "Recreate this pending item or run deferred-session recovery.",
            debugDetail: detail
        )
    }

    private static let iso8601FormatterWithFractional: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private static let iso8601Formatter = ISO8601DateFormatter()
}
