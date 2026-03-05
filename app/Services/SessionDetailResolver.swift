import Foundation

public enum TranscriptResolutionSource: String, Codable, Sendable {
    case manifest
    case jsonl
    case unavailable
}

public enum SessionConversationState: String, Codable, Sendable {
    case ready
    case empty
}

public struct SessionDetailDTO: Equatable, Sendable {
    public var summary: SessionSummaryDTO
    public var manifestPath: URL?
    public var jsonlPath: URL?
    public var wavPath: URL?
    public var audioAvailable: Bool
    public var trustNoticeCount: Int
    public var transcriptSource: TranscriptResolutionSource
    public var conversationState: SessionConversationState
    public var conversationLines: [SessionConversationLine]
    public var malformedJsonlLineCount: Int

    public init(
        summary: SessionSummaryDTO,
        manifestPath: URL?,
        jsonlPath: URL?,
        wavPath: URL?,
        audioAvailable: Bool,
        trustNoticeCount: Int,
        transcriptSource: TranscriptResolutionSource,
        conversationState: SessionConversationState,
        conversationLines: [SessionConversationLine],
        malformedJsonlLineCount: Int
    ) {
        self.summary = summary
        self.manifestPath = manifestPath
        self.jsonlPath = jsonlPath
        self.wavPath = wavPath
        self.audioAvailable = audioAvailable
        self.trustNoticeCount = trustNoticeCount
        self.transcriptSource = transcriptSource
        self.conversationState = conversationState
        self.conversationLines = conversationLines
        self.malformedJsonlLineCount = malformedJsonlLineCount
    }
}

public struct SessionDetailResolver {
    private let timelineResolver: TranscriptTimelineResolver
    private let jsonlDecoder: JsonlTranscriptDecoder
    private let fileManager: FileManager

    public init(
        timelineResolver: TranscriptTimelineResolver = TranscriptTimelineResolver(),
        jsonlDecoder: JsonlTranscriptDecoder = JsonlTranscriptDecoder(),
        fileManager: FileManager = .default
    ) {
        self.timelineResolver = timelineResolver
        self.jsonlDecoder = jsonlDecoder
        self.fileManager = fileManager
    }

    public func resolve(session summary: SessionSummaryDTO) -> SessionDetailDTO {
        let sessionRoot = summary.rootPath.standardizedFileURL
        let defaultManifestPath = sessionRoot.appendingPathComponent("session.manifest.json")
        let defaultJsonlPath = sessionRoot.appendingPathComponent("session.jsonl")
        let defaultWavPath = sessionRoot.appendingPathComponent("session.wav")

        var resolvedLines: [SessionConversationLine] = []
        var source: TranscriptResolutionSource = .unavailable
        var trustNoticeCount = 0
        var malformedJsonlLineCount = 0

        let manifestSurface = parseManifestSurface(at: defaultManifestPath, sessionRoot: sessionRoot)
        let manifestPath = manifestSurface?.manifestPath
        let jsonlFromManifest = manifestSurface?.jsonlPath

        var wavPath = manifestSurface?.wavPath
        if wavPath == nil, fileManager.fileExists(atPath: defaultWavPath.path) {
            wavPath = defaultWavPath
        }

        if let manifestSurface, !manifestSurface.lines.isEmpty {
            source = .manifest
            trustNoticeCount = manifestSurface.trustNoticeCount
            resolvedLines = manifestSurface.lines
        }

        if resolvedLines.isEmpty {
            let jsonlCandidates = orderedJsonlCandidates(primary: jsonlFromManifest, fallback: defaultJsonlPath)
            for candidate in jsonlCandidates {
                do {
                    let decoded = try jsonlDecoder.decodeStableTranscript(at: candidate)
                    malformedJsonlLineCount = decoded.malformedLineCount
                    if !decoded.lines.isEmpty {
                        resolvedLines = decoded.lines
                        source = .jsonl
                        break
                    }
                } catch {
                    continue
                }
            }
        }

        let jsonlPath = firstExistingJsonlPath(primary: jsonlFromManifest, fallback: defaultJsonlPath)
        let conversationState: SessionConversationState = resolvedLines.isEmpty ? .empty : .ready

        return SessionDetailDTO(
            summary: summary,
            manifestPath: manifestPath,
            jsonlPath: jsonlPath,
            wavPath: wavPath,
            audioAvailable: wavPath.map { fileManager.fileExists(atPath: $0.path) } ?? false,
            trustNoticeCount: trustNoticeCount,
            transcriptSource: source,
            conversationState: conversationState,
            conversationLines: resolvedLines,
            malformedJsonlLineCount: malformedJsonlLineCount
        )
    }

    private func parseManifestSurface(at manifestPath: URL, sessionRoot: URL) -> ManifestTranscriptSurface? {
        let resolvedManifestPath = manifestPath.standardizedFileURL
        guard fileManager.fileExists(atPath: resolvedManifestPath.path) else {
            return nil
        }
        let data: Data
        do {
            let handle = try FileHandle(forReadingFrom: resolvedManifestPath)
            defer {
                try? handle.close()
            }
            data = try handle.readToEnd() ?? Data()
        } catch {
            return nil
        }

        guard let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }

        let eventObjects = payload["events"] as? [[String: Any]] ?? []
        var lines = timelineResolver.canonicalDisplayLines(
            from: eventObjects.compactMap(TranscriptTimelineResolver.parseTranscriptLine)
        )

        // Some manifests include stable transcript text in terminal_summary without event rows.
        if lines.isEmpty {
            let stableLines = parseStableSummaryLines(payload)
            lines = timelineResolver.canonicalDisplayLines(
                from: stableLines.enumerated().map { index, text in
                    SessionConversationLine(
                        eventType: .final,
                        channel: "mixed",
                        segmentID: "stable-\(index)",
                        startMs: UInt64(index * 1000),
                        endMs: UInt64(index * 1000 + 1),
                        text: text
                    )
                }
            )
        }

        let trustNoticeCount = parseTrustNoticeCount(payload)
        let jsonlPath = resolveOptionalPath(payload["jsonl_path"] as? String, sessionRoot: sessionRoot)
        let wavPath = resolveOptionalPath(payload["out_wav"] as? String, sessionRoot: sessionRoot)

        return ManifestTranscriptSurface(
            manifestPath: resolvedManifestPath,
            jsonlPath: jsonlPath,
            wavPath: wavPath,
            trustNoticeCount: trustNoticeCount,
            lines: lines
        )
    }

    private func parseStableSummaryLines(_ payload: [String: Any]) -> [String] {
        guard
            let terminalSummary = payload["terminal_summary"] as? [String: Any],
            let stableLines = terminalSummary["stable_lines"] as? [String]
        else {
            return []
        }
        return stableLines
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    private func parseTrustNoticeCount(_ payload: [String: Any]) -> Int {
        guard let trust = payload["trust"] as? [String: Any] else {
            return 0
        }

        if let count = trust["notice_count"] as? Int {
            return count
        }
        if let count = trust["notice_count"] as? NSNumber {
            return max(0, count.intValue)
        }
        if let notices = trust["notices"] as? [[String: Any]] {
            return notices.count
        }
        return 0
    }

    private func orderedJsonlCandidates(primary: URL?, fallback: URL) -> [URL] {
        var candidates: [URL] = []
        if let primary {
            candidates.append(primary.standardizedFileURL)
        }
        candidates.append(fallback.standardizedFileURL)

        var deduped: [URL] = []
        var seen = Set<String>()
        for candidate in candidates {
            if seen.insert(candidate.path).inserted {
                deduped.append(candidate)
            }
        }
        return deduped
    }

    private func firstExistingJsonlPath(primary: URL?, fallback: URL) -> URL? {
        for candidate in orderedJsonlCandidates(primary: primary, fallback: fallback) {
            if fileManager.fileExists(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }

    private func resolveOptionalPath(_ rawPath: String?, sessionRoot: URL) -> URL? {
        guard let rawPath = rawPath?.trimmingCharacters(in: .whitespacesAndNewlines), !rawPath.isEmpty else {
            return nil
        }

        let url: URL
        if rawPath.hasPrefix("/") {
            url = URL(fileURLWithPath: rawPath)
        } else {
            url = sessionRoot.appendingPathComponent(rawPath)
        }
        return url.standardizedFileURL
    }
}

private struct ManifestTranscriptSurface {
    var manifestPath: URL
    var jsonlPath: URL?
    var wavPath: URL?
    var trustNoticeCount: Int
    var lines: [SessionConversationLine]
}
