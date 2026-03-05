import Foundation

public final class SessionTranscriptSearchIndex {
    private struct ArtifactFingerprint: Equatable {
        let manifestSignature: String?
        let jsonlSignature: String?
    }

    private struct IndexedTranscriptEntry {
        let fingerprint: ArtifactFingerprint
        let searchableTranscript: String
    }

    private let lock = NSLock()
    private var entriesBySessionPath: [String: IndexedTranscriptEntry] = [:]

    public init() {}

    public func searchSessionPaths(
        sessions: [SessionSummaryDTO],
        query: String,
        fileManager: FileManager = .default
    ) -> Set<String> {
        let normalizedQuery = query
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        guard !normalizedQuery.isEmpty else {
            return []
        }

        lock.lock()
        defer { lock.unlock() }

        let livePaths = Set(sessions.map { $0.rootPath.standardizedFileURL.path })
        if !livePaths.isEmpty {
            entriesBySessionPath = entriesBySessionPath.filter { livePaths.contains($0.key) }
        } else {
            entriesBySessionPath.removeAll(keepingCapacity: false)
            return []
        }

        var matches = Set<String>()
        matches.reserveCapacity(sessions.count)

        for session in sessions {
            let rootPath = session.rootPath.standardizedFileURL.path
            let rootURL = session.rootPath.standardizedFileURL
            let fingerprint = artifactFingerprint(sessionRoot: rootURL, fileManager: fileManager)

            let transcript: String
            if let existing = entriesBySessionPath[rootPath], existing.fingerprint == fingerprint {
                transcript = existing.searchableTranscript
            } else {
                transcript = transcriptText(sessionRoot: rootURL, fileManager: fileManager)
                    .lowercased()
                entriesBySessionPath[rootPath] = IndexedTranscriptEntry(
                    fingerprint: fingerprint,
                    searchableTranscript: transcript
                )
            }

            if transcript.contains(normalizedQuery) {
                matches.insert(rootPath)
            }
        }

        return matches
    }

    private func artifactFingerprint(sessionRoot: URL, fileManager: FileManager) -> ArtifactFingerprint {
        ArtifactFingerprint(
            manifestSignature: fileSignature(sessionRoot.appendingPathComponent("session.manifest.json"), fileManager: fileManager),
            jsonlSignature: fileSignature(sessionRoot.appendingPathComponent("session.jsonl"), fileManager: fileManager)
        )
    }

    private func fileSignature(_ path: URL, fileManager: FileManager) -> String? {
        guard let attributes = try? fileManager.attributesOfItem(atPath: path.path) else {
            return nil
        }

        let size = (attributes[.size] as? NSNumber)?.int64Value ?? -1
        let modified = (attributes[.modificationDate] as? Date)?.timeIntervalSince1970 ?? -1
        return "\(size):\(modified)"
    }

    private func transcriptText(sessionRoot: URL, fileManager: FileManager) -> String {
        let manifestURL = sessionRoot.appendingPathComponent("session.manifest.json")
        if fileManager.fileExists(atPath: manifestURL.path),
           let manifestData = try? readData(at: manifestURL),
           let manifestText = transcriptFromManifest(manifestData),
           !manifestText.isEmpty {
            return manifestText
        }

        let jsonlURL = sessionRoot.appendingPathComponent("session.jsonl")
        if fileManager.fileExists(atPath: jsonlURL.path),
           let jsonlData = try? readData(at: jsonlURL),
           let jsonlText = transcriptFromJsonl(jsonlData),
           !jsonlText.isEmpty {
            return jsonlText
        }

        return ""
    }

    private func transcriptFromManifest(_ data: Data) -> String? {
        guard let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }

        if let terminalSummary = payload["terminal_summary"] as? [String: Any],
           let stableLines = terminalSummary["stable_lines"] as? [String],
           !stableLines.isEmpty {
            return stableLines.joined(separator: "\n")
        }

        if let transcript = payload["transcript"] as? String,
           !transcript.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return transcript
        }

        return nil
    }

    private func transcriptFromJsonl(_ data: Data) -> String? {
        guard let text = String(data: data, encoding: .utf8) else {
            return nil
        }

        var reconciled = [String]()
        var llm = [String]()
        var final = [String]()

        for rawLine in text.split(separator: "\n", omittingEmptySubsequences: true) {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !line.isEmpty,
                  let lineData = line.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any],
                  let eventType = object["event_type"] as? String,
                  let transcriptText = object["text"] as? String else {
                continue
            }

            let clean = transcriptText.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !clean.isEmpty else { continue }

            switch eventType {
            case "reconciled_final":
                reconciled.append(clean)
            case "llm_final":
                llm.append(clean)
            case "final":
                final.append(clean)
            default:
                break
            }
        }

        let selected: [String]
        if !reconciled.isEmpty {
            selected = reconciled
        } else if !llm.isEmpty {
            selected = llm
        } else {
            selected = final
        }

        guard !selected.isEmpty else {
            return nil
        }
        return selected.joined(separator: "\n")
    }

    private func readData(at url: URL) throws -> Data {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.readToEnd() ?? Data()
    }
}
