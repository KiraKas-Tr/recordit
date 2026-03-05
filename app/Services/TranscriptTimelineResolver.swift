import Foundation

public enum SessionTranscriptEventType: String, Codable, Sendable {
    case partial
    case final
    case reconciledFinal = "reconciled_final"
    case llmFinal = "llm_final"

    var rank: Int {
        switch self {
        case .partial:
            return 0
        case .final:
            return 1
        case .reconciledFinal:
            return 2
        case .llmFinal:
            return 3
        }
    }

    var isStableForSessionDetail: Bool {
        self != .partial
    }
}

public struct SessionConversationLine: Equatable, Sendable {
    public var eventType: SessionTranscriptEventType
    public var channel: String
    public var segmentID: String
    public var sourceFinalSegmentID: String?
    public var startMs: UInt64
    public var endMs: UInt64
    public var text: String

    public init(
        eventType: SessionTranscriptEventType,
        channel: String,
        segmentID: String,
        sourceFinalSegmentID: String? = nil,
        startMs: UInt64,
        endMs: UInt64,
        text: String
    ) {
        self.eventType = eventType
        self.channel = channel
        self.segmentID = segmentID
        self.sourceFinalSegmentID = sourceFinalSegmentID
        self.startMs = startMs
        self.endMs = endMs
        self.text = text
    }

    var orderingKey: SessionConversationOrderingKey {
        SessionConversationOrderingKey(
            startMs: startMs,
            endMs: endMs,
            eventTypeRank: eventType.rank,
            channel: channel,
            segmentID: segmentID,
            sourceFinalSegmentID: sourceFinalSegmentID ?? "",
            text: text
        )
    }
}

public struct SessionConversationOrderingKey: Comparable, Hashable, Sendable {
    public var startMs: UInt64
    public var endMs: UInt64
    public var eventTypeRank: Int
    public var channel: String
    public var segmentID: String
    public var sourceFinalSegmentID: String
    public var text: String

    public static func < (lhs: Self, rhs: Self) -> Bool {
        if lhs.startMs != rhs.startMs {
            return lhs.startMs < rhs.startMs
        }
        if lhs.endMs != rhs.endMs {
            return lhs.endMs < rhs.endMs
        }
        if lhs.eventTypeRank != rhs.eventTypeRank {
            return lhs.eventTypeRank < rhs.eventTypeRank
        }
        if lhs.channel != rhs.channel {
            return lhs.channel < rhs.channel
        }
        if lhs.segmentID != rhs.segmentID {
            return lhs.segmentID < rhs.segmentID
        }
        if lhs.sourceFinalSegmentID != rhs.sourceFinalSegmentID {
            return lhs.sourceFinalSegmentID < rhs.sourceFinalSegmentID
        }
        return lhs.text < rhs.text
    }
}

public struct TranscriptTimelineResolver {
    public init() {}

    public func canonicalDisplayLines(from rawLines: [SessionConversationLine]) -> [SessionConversationLine] {
        let sorted = rawLines.sorted { $0.orderingKey < $1.orderingKey }
        let deduplicated = deduplicate(sorted)
        let preferred = applyReconciledPreference(to: deduplicated)

        return preferred
            .filter { $0.eventType.isStableForSessionDetail }
            .sorted { $0.orderingKey < $1.orderingKey }
    }

    public static func parseTranscriptLine(from payload: [String: Any]) -> SessionConversationLine? {
        guard
            let eventTypeRaw = payload["event_type"] as? String,
            let eventType = SessionTranscriptEventType(rawValue: eventTypeRaw)
        else {
            return nil
        }

        let channel = (payload["channel"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let segmentID = (payload["segment_id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let text = (payload["text"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)

        guard
            let channel,
            !channel.isEmpty,
            let segmentID,
            !segmentID.isEmpty,
            let text,
            !text.isEmpty,
            let startMs = parseUnsignedInteger(payload["start_ms"]),
            let endMs = parseUnsignedInteger(payload["end_ms"])
        else {
            return nil
        }

        return SessionConversationLine(
            eventType: eventType,
            channel: channel,
            segmentID: segmentID,
            sourceFinalSegmentID: payload["source_final_segment_id"] as? String,
            startMs: startMs,
            endMs: endMs,
            text: text
        )
    }

    private func applyReconciledPreference(to lines: [SessionConversationLine]) -> [SessionConversationLine] {
        let reconciledLines = lines.filter { $0.eventType == .reconciledFinal }
        guard !reconciledLines.isEmpty else {
            return lines
        }

        let preferredFinalIDs = Set(reconciledLines.map { $0.sourceFinalSegmentID ?? $0.segmentID })
        return lines.filter { line in
            guard line.eventType == .final else {
                return true
            }
            return !preferredFinalIDs.contains(line.segmentID)
        }
    }

    private func deduplicate(_ lines: [SessionConversationLine]) -> [SessionConversationLine] {
        var seen = Set<SessionConversationOrderingKey>()
        var result: [SessionConversationLine] = []
        for line in lines {
            let key = line.orderingKey
            if seen.insert(key).inserted {
                result.append(line)
            }
        }
        return result
    }

    private static func parseUnsignedInteger(_ value: Any?) -> UInt64? {
        switch value {
        case let integer as UInt64:
            return integer
        case let integer as Int:
            return integer >= 0 ? UInt64(integer) : nil
        case let number as NSNumber:
            return number.int64Value >= 0 ? UInt64(number.int64Value) : nil
        case let string as String:
            return UInt64(string)
        default:
            return nil
        }
    }
}
