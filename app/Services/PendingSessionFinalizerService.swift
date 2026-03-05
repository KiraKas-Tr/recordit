import Foundation

public protocol PendingSessionFinalizing: Sendable {
    func finalizePendingSession(sessionRoot: URL) throws
}

public struct PendingSessionFinalizerService: PendingSessionFinalizing {
    public typealias DataReader = @Sendable (URL) throws -> Data
    public typealias FileExists = @Sendable (URL) -> Bool
    public typealias RemoveItem = @Sendable (URL) throws -> Void

    private let dataReader: DataReader
    private let fileExists: FileExists
    private let removeItem: RemoveItem

    public init(
        dataReader: @escaping DataReader = {
            let handle = try FileHandle(forReadingFrom: $0)
            defer { try? handle.close() }
            return try handle.readToEnd() ?? Data()
        },
        fileExists: @escaping FileExists = { FileManager.default.fileExists(atPath: $0.path) },
        removeItem: @escaping RemoveItem = { try FileManager.default.removeItem(at: $0) }
    ) {
        self.dataReader = dataReader
        self.fileExists = fileExists
        self.removeItem = removeItem
    }

    public func finalizePendingSession(sessionRoot: URL) throws {
        let root = sessionRoot.standardizedFileURL
        let manifestURL = root.appendingPathComponent("session.manifest.json")
        let pendingURL = root.appendingPathComponent("session.pending.json")
        let retryContextURL = root.appendingPathComponent("session.pending.retry.json")

        guard fileExists(manifestURL) else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Deferred session finalization is missing a manifest.",
                remediation: "Retry deferred transcription to regenerate the manifest.",
                debugDetail: manifestURL.path
            )
        }
        _ = try parseManifestStatus(manifestURL)

        if fileExists(pendingURL) {
            do {
                try removeItem(pendingURL)
            } catch {
                throw AppServiceError(
                    code: .ioFailure,
                    userMessage: "Could not remove pending-session sidecar.",
                    remediation: "Verify session folder permissions and retry finalization.",
                    debugDetail: "\(pendingURL.path): \(error)"
                )
            }
        }

        if fileExists(retryContextURL) {
            try? removeItem(retryContextURL)
        }
    }

    private func parseManifestStatus(_ manifestURL: URL) throws -> String {
        let data = try dataReader(manifestURL)
        guard let payload = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let summary = payload["session_summary"] as? [String: Any],
              let status = summary["session_status"] as? String else {
            throw AppServiceError(
                code: .manifestInvalid,
                userMessage: "Session manifest is invalid.",
                remediation: "Re-run deferred transcription to regenerate canonical artifacts.",
                debugDetail: manifestURL.path
            )
        }
        return status
    }
}
