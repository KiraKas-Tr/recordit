import Foundation

public struct FileSystemArtifactIntegrityService: ArtifactIntegrityService {
    public typealias DataReader = @Sendable (URL) throws -> Data

    private let dataReader: DataReader

    public init(
        dataReader: @escaping DataReader = { url in
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            return try handle.readToEnd() ?? Data()
        }
    ) {
        self.dataReader = dataReader
    }

    public func evaluateSessionArtifacts(
        sessionID: String,
        rootPath: URL
    ) throws -> SessionArtifactIntegrityReportDTO {
        let root = rootPath.standardizedFileURL
        let resolvedSessionID = sessionID.isEmpty ? root.lastPathComponent : sessionID
        var findings: [ArtifactIntegrityFindingDTO] = []

        guard directoryExists(at: root) else {
            findings.append(
                finding(
                    code: "session_root_missing",
                    summary: "Session folder is missing.",
                    remediation: "Refresh the sessions list and retry. If the folder was moved manually, restore it first.",
                    disposition: .terminal,
                    diagnostics: [
                        "root_path": root.path
                    ]
                )
            )
            return report(
                sessionID: resolvedSessionID,
                rootPath: root,
                findings: findings
            )
        }

        let manifestURL = root.appendingPathComponent("session.manifest.json")
        let pendingURL = root.appendingPathComponent("session.pending.json")
        let wavURL = root.appendingPathComponent("session.wav")
        let jsonlURL = root.appendingPathComponent("session.jsonl")

        let hasManifest = FileManager.default.fileExists(atPath: manifestURL.path)
        let hasPending = FileManager.default.fileExists(atPath: pendingURL.path)
        let hasWav = FileManager.default.fileExists(atPath: wavURL.path)
        let hasJsonl = FileManager.default.fileExists(atPath: jsonlURL.path)

        if !hasManifest && !hasPending {
            findings.append(
                finding(
                    code: "missing_manifest_and_pending_sidecar",
                    summary: "Session metadata is missing.",
                    remediation: "This session cannot be resolved safely. Restore metadata from backup or remove the broken entry.",
                    disposition: .terminal,
                    diagnostics: [
                        "manifest_path": manifestURL.path,
                        "pending_path": pendingURL.path
                    ]
                )
            )
        }

        if hasPending && !hasWav {
            findings.append(
                finding(
                    code: "pending_sidecar_without_audio",
                    summary: "Pending sidecar exists but required audio is missing.",
                    remediation: "Restore `session.wav` or discard this pending item.",
                    disposition: .terminal,
                    diagnostics: [
                        "pending_path": pendingURL.path,
                        "wav_path": wavURL.path
                    ]
                )
            )
        }

        if hasPending, let pendingError = pendingSidecarParseError(at: pendingURL) {
            findings.append(
                finding(
                    code: "pending_sidecar_invalid",
                    summary: "Pending sidecar is malformed.",
                    remediation: "Regenerate `session.pending.json` using the record-only pending writer.",
                    disposition: .recoverable,
                    diagnostics: [
                        "pending_path": pendingURL.path,
                        "error": pendingError
                    ]
                )
            )
        }

        if hasManifest && !hasWav {
            findings.append(
                finding(
                    code: "manifest_without_audio",
                    summary: "Session audio is missing.",
                    remediation: "Restore `session.wav` from backup. Transcript details may still be available.",
                    disposition: .recoverable,
                    diagnostics: [
                        "manifest_path": manifestURL.path,
                        "wav_path": wavURL.path
                    ]
                )
            )
        }

        if hasManifest {
            if let parseError = manifestParseError(at: manifestURL) {
                findings.append(
                    finding(
                        code: "manifest_invalid_json",
                        summary: "Manifest exists but is not valid JSON.",
                        remediation: hasPending && hasWav
                            ? "Use pending-session recovery to rebuild final artifacts."
                            : "Restore a valid manifest from backup.",
                        disposition: hasPending && hasWav ? .recoverable : .terminal,
                        diagnostics: [
                            "manifest_path": manifestURL.path,
                            "error": parseError
                        ]
                    )
                )
            }
        }

        if hasJsonl, let jsonlError = jsonlParseError(at: jsonlURL) {
            findings.append(
                finding(
                    code: "jsonl_corrupt",
                    summary: "Transcript event stream is malformed.",
                    remediation: "Use manifest transcript fallback or rerun transcript reconstruction.",
                    disposition: .recoverable,
                    diagnostics: [
                        "jsonl_path": jsonlURL.path,
                        "error": jsonlError
                    ]
                )
            )
        }

        return report(
            sessionID: resolvedSessionID,
            rootPath: root,
            findings: findings
        )
    }

    private func report(
        sessionID: String,
        rootPath: URL,
        findings: [ArtifactIntegrityFindingDTO]
    ) -> SessionArtifactIntegrityReportDTO {
        let state: ArtifactIntegrityState
        if findings.isEmpty {
            state = .healthy
        } else if findings.contains(where: { $0.disposition == .terminal }) {
            state = .terminal
        } else {
            state = .recoverable
        }
        return SessionArtifactIntegrityReportDTO(
            sessionID: sessionID,
            rootPath: rootPath,
            state: state,
            findings: findings
        )
    }

    private func directoryExists(at url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)
            && isDirectory.boolValue
    }

    private func manifestParseError(at manifestURL: URL) -> String? {
        do {
            let data = try dataReader(manifestURL)
            _ = try JSONSerialization.jsonObject(with: data)
            return nil
        } catch {
            return String(describing: error)
        }
    }

    private func jsonlParseError(at jsonlURL: URL) -> String? {
        let raw: String
        do {
            raw = String(decoding: try dataReader(jsonlURL), as: UTF8.self)
        } catch {
            return String(describing: error)
        }

        let lines = raw.split(whereSeparator: \.isNewline)
        for (index, line) in lines.enumerated() {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty {
                continue
            }
            guard let data = trimmed.data(using: .utf8),
                  (try? JSONSerialization.jsonObject(with: data)) != nil else {
                return "invalid JSON at line \(index + 1)"
            }
        }
        return nil
    }

    private func pendingSidecarParseError(at pendingURL: URL) -> String? {
        do {
            let data = try dataReader(pendingURL)
            let sidecar = try JSONDecoder().decode(PendingSessionSidecarDTO.self, from: data)
            if sidecar.sessionID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return "missing session_id"
            }
            if sidecar.createdAtUTC.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return "missing created_at_utc"
            }
            if Self.parseISO8601(sidecar.createdAtUTC) == nil {
                return "invalid created_at_utc"
            }
            if sidecar.mode != .recordOnly {
                return "invalid mode=\(sidecar.mode.rawValue)"
            }
            let wavPath = sidecar.wavPath.trimmingCharacters(in: .whitespacesAndNewlines)
            if wavPath.isEmpty || !wavPath.hasPrefix("/") {
                return "invalid wav_path"
            }
            return nil
        } catch {
            return String(describing: error)
        }
    }

    private static func parseISO8601(_ value: String) -> Date? {
        if let date = iso8601WithFractionalSeconds.date(from: value) {
            return date
        }
        return iso8601Basic.date(from: value)
    }

    private static let iso8601WithFractionalSeconds: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private static let iso8601Basic: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()

    private func finding(
        code: String,
        summary: String,
        remediation: String,
        disposition: ArtifactIntegrityDisposition,
        diagnostics: [String: String]
    ) -> ArtifactIntegrityFindingDTO {
        ArtifactIntegrityFindingDTO(
            code: code,
            summary: summary,
            remediation: remediation,
            disposition: disposition,
            diagnostics: diagnostics
        )
    }
}
