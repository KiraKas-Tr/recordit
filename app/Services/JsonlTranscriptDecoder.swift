import Foundation

public struct JsonlTranscriptDecodeResult: Equatable, Sendable {
    public var lines: [SessionConversationLine]
    public var malformedLineCount: Int

    public init(lines: [SessionConversationLine], malformedLineCount: Int) {
        self.lines = lines
        self.malformedLineCount = malformedLineCount
    }
}

public struct JsonlTranscriptDecoder {
    private let timelineResolver: TranscriptTimelineResolver

    public init(timelineResolver: TranscriptTimelineResolver = TranscriptTimelineResolver()) {
        self.timelineResolver = timelineResolver
    }

    public func decodeStableTranscript(at jsonlPath: URL) throws -> JsonlTranscriptDecodeResult {
        let resolvedPath = jsonlPath.standardizedFileURL
        guard FileManager.default.fileExists(atPath: resolvedPath.path) else {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "Session transcript file is missing.",
                remediation: "Open the session folder and verify `session.jsonl` exists.",
                debugDetail: resolvedPath.path
            )
        }

        let rawData: Data
        do {
            let handle = try FileHandle(forReadingFrom: resolvedPath)
            defer {
                try? handle.close()
            }
            rawData = try handle.readToEnd() ?? Data()
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not read session transcript data.",
                remediation: "Retry opening the session detail.",
                debugDetail: String(describing: error)
            )
        }

        guard let rawText = String(data: rawData, encoding: .utf8) else {
            throw AppServiceError(
                code: .jsonlCorrupt,
                userMessage: "Session transcript data is unreadable.",
                remediation: "Re-run replay or regenerate artifacts for this session.",
                debugDetail: resolvedPath.path
            )
        }

        var completeLines = rawText.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if !rawText.hasSuffix("\n"), !completeLines.isEmpty {
            _ = completeLines.removeLast()
        }

        var parsedLines: [SessionConversationLine] = []
        var malformedLineCount = 0

        for line in completeLines {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty {
                continue
            }
            guard let lineData = trimmed.data(using: .utf8) else {
                malformedLineCount += 1
                continue
            }
            guard
                let object = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any],
                let parsed = TranscriptTimelineResolver.parseTranscriptLine(from: object)
            else {
                malformedLineCount += 1
                continue
            }
            parsedLines.append(parsed)
        }

        return JsonlTranscriptDecodeResult(
            lines: timelineResolver.canonicalDisplayLines(from: parsedLines),
            malformedLineCount: malformedLineCount
        )
    }
}
