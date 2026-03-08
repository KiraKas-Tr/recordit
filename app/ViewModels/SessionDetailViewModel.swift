import Foundation

@MainActor
public final class SessionDetailViewModel {
    public enum LoadState: Equatable {
        case idle
        case loading
        case loaded(SessionDetailDTO)
    }

    public private(set) var state: LoadState = .idle

    public static let accessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "session_header",
            label: "Session summary",
            hint: "Announces session metadata and status."
        ),
        AccessibilityElementDescriptor(
            id: "conversation_timeline",
            label: "Conversation timeline",
            hint: "Read stable transcript lines in chronological order."
        ),
        AccessibilityElementDescriptor(
            id: "playback_controls",
            label: "Playback controls",
            hint: "Play, pause, and seek session audio."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: ["session_header", "conversation_timeline", "playback_controls"]
    )

    public static let keyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "focus_timeline_shortcut",
            key: "t",
            modifiers: ["command"],
            actionSummary: "Move keyboard focus to the conversation timeline."
        ),
    ]

    private let resolver: SessionDetailResolver

    public init(resolver: SessionDetailResolver = SessionDetailResolver()) {
        self.resolver = resolver
    }

    public func load(session summary: SessionSummaryDTO) {
        state = .loading
        state = .loaded(resolver.resolve(session: summary))
    }
}
