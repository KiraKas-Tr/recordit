import Foundation

public struct FileSystemJsonlTailService: JsonlTailService {
    public init() {}

    public func readEvents(at jsonlPath: URL, from cursor: JsonlTailCursor) throws -> ([RuntimeEventDTO], JsonlTailCursor) {
        let path = jsonlPath.standardizedFileURL
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: path.path) else {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "Transcript stream file is missing.",
                remediation: "Wait for session output to materialize and retry."
            )
        }

        let attributes: [FileAttributeKey: Any]
        do {
            attributes = try fileManager.attributesOfItem(atPath: path.path)
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not read transcript stream metadata.",
                remediation: "Verify file permissions and retry.",
                debugDetail: String(describing: error)
            )
        }

        let fileSize = (attributes[.size] as? NSNumber)?.uint64Value ?? 0
        let modifiedAt = attributes[.modificationDate] as? Date

        var startOffset = cursor.byteOffset
        var startLineCount = cursor.lineCount

        if startOffset > fileSize {
            startOffset = 0
            startLineCount = 0
        }
        if let previousModifiedAt = cursor.lastModifiedAt,
           let currentModifiedAt = modifiedAt,
           currentModifiedAt < previousModifiedAt {
            startOffset = 0
            startLineCount = 0
        }

        let handle: FileHandle
        do {
            handle = try FileHandle(forReadingFrom: path)
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not open transcript stream.",
                remediation: "Verify file permissions and retry.",
                debugDetail: String(describing: error)
            )
        }
        defer { try? handle.close() }

        do {
            try handle.seek(toOffset: startOffset)
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not seek transcript stream.",
                remediation: "Re-open the session and retry transcript streaming.",
                debugDetail: String(describing: error)
            )
        }

        let unreadData: Data
        do {
            unreadData = try handle.readToEnd() ?? Data()
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not read transcript stream.",
                remediation: "Retry transcript streaming.",
                debugDetail: String(describing: error)
            )
        }

        guard !unreadData.isEmpty else {
            return ([], JsonlTailCursor(byteOffset: startOffset, lineCount: startLineCount, lastModifiedAt: modifiedAt))
        }

        guard let lastNewline = unreadData.lastIndex(of: 0x0A) else {
            return ([], JsonlTailCursor(byteOffset: startOffset, lineCount: startLineCount, lastModifiedAt: modifiedAt))
        }

        let consumedLength = unreadData.distance(from: unreadData.startIndex, to: unreadData.index(after: lastNewline))
        let consumedData = unreadData.prefix(consumedLength)
        let parsed = parseCompleteLines(consumedData)

        let nextOffset = startOffset + UInt64(consumedLength)
        let nextCursor = JsonlTailCursor(
            byteOffset: nextOffset,
            lineCount: startLineCount + parsed.completeLineCount,
            lastModifiedAt: modifiedAt
        )
        return (parsed.events, nextCursor)
    }

    private func parseCompleteLines(_ data: Data) -> (events: [RuntimeEventDTO], completeLineCount: UInt64) {
        var events = [RuntimeEventDTO]()
        events.reserveCapacity(32)

        var lineStart = data.startIndex
        var completeLineCount: UInt64 = 0

        for index in data.indices where data[index] == 0x0A {
            let lineData = Data(data[lineStart..<index])
            lineStart = data.index(after: index)
            completeLineCount += 1

            if let event = parseLine(lineData) {
                events.append(event)
            }
        }

        return (events, completeLineCount)
    }

    private func parseLine(_ lineData: Data) -> RuntimeEventDTO? {
        var trimmed = lineData
        while let byte = trimmed.last, byte == 0x0D {
            trimmed.removeLast()
        }
        guard !trimmed.isEmpty else {
            return nil
        }

        guard let object = try? JSONSerialization.jsonObject(with: trimmed) as? [String: Any],
              let eventType = object["event_type"] as? String,
              !eventType.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }

        let channel = object["channel"] as? String
        let segmentID = object["segment_id"] as? String
        let startMs = asUInt64(object["start_ms"])
        let endMs = asUInt64(object["end_ms"])
        let text = object["text"] as? String

        let reserved: Set<String> = ["event_type", "channel", "segment_id", "start_ms", "end_ms", "text", "payload"]
        var payload = [String: String]()
        if let nestedPayload = object["payload"] as? [String: Any] {
            for key in nestedPayload.keys.sorted() {
                payload[key] = stringify(nestedPayload[key])
            }
        }
        for key in object.keys.sorted() where !reserved.contains(key) {
            payload[key] = stringify(object[key])
        }

        return RuntimeEventDTO(
            eventType: eventType,
            channel: channel,
            segmentID: segmentID,
            startMs: startMs,
            endMs: endMs,
            text: text,
            payload: payload
        )
    }

    private func asUInt64(_ value: Any?) -> UInt64? {
        if let number = value as? NSNumber {
            return number.uint64Value
        }
        if let text = value as? String {
            return UInt64(text)
        }
        return nil
    }

    private func stringify(_ value: Any?) -> String {
        guard let value else {
            return "null"
        }
        if let string = value as? String {
            return string
        }
        if JSONSerialization.isValidJSONObject(["value": value]),
           let data = try? JSONSerialization.data(withJSONObject: ["value": value], options: [.sortedKeys]),
           let encoded = String(data: data, encoding: .utf8) {
            return encoded
        }
        return String(describing: value)
    }
}
