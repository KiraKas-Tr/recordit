import Foundation

@MainActor
public final class SessionListViewModel {
    public enum ModeFilter: String, CaseIterable, Sendable {
        case all
        case live
        case recordOnly = "record_only"

        var runtimeMode: RuntimeMode? {
            switch self {
            case .all:
                return nil
            case .live:
                return .live
            case .recordOnly:
                return .recordOnly
            }
        }
    }

    public enum StatusFilter: String, CaseIterable, Sendable {
        case all
        case pending
        case ok
        case degraded
        case failed

        var sessionStatus: SessionStatus? {
            switch self {
            case .all:
                return nil
            case .pending:
                return .pending
            case .ok:
                return .ok
            case .degraded:
                return .degraded
            case .failed:
                return .failed
            }
        }
    }

    public struct Filters: Equatable, Sendable {
        public var mode: ModeFilter
        public var status: StatusFilter
        public var searchText: String

        public init(
            mode: ModeFilter = .all,
            status: StatusFilter = .all,
            searchText: String = ""
        ) {
            self.mode = mode
            self.status = status
            self.searchText = searchText
        }
    }

    public struct EmptyState: Equatable, Sendable {
        public var title: String
        public var detail: String

        public init(title: String, detail: String) {
            self.title = title
            self.detail = detail
        }
    }

    public enum ViewState: Equatable {
        case idle
        case loading(previousItems: [SessionSummaryDTO])
        case loaded(items: [SessionSummaryDTO])
        case empty(EmptyState)
        case failed(AppServiceError, recoverableItems: [SessionSummaryDTO])
    }

    public private(set) var state: ViewState = .idle
    public private(set) var filters = Filters()

    public static let accessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "sessions_search",
            label: "Search sessions",
            hint: "Filter sessions by ID or transcript text."
        ),
        AccessibilityElementDescriptor(
            id: "sessions_mode_filter",
            label: "Session mode filter",
            hint: "Filter sessions by all, live, or record-only mode."
        ),
        AccessibilityElementDescriptor(
            id: "sessions_status_filter",
            label: "Session status filter",
            hint: "Filter sessions by pending, ok, degraded, or failed status."
        ),
        AccessibilityElementDescriptor(
            id: "sessions_results_list",
            label: "Sessions list",
            hint: "Navigate session rows in newest-first order."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: [
            "sessions_search",
            "sessions_mode_filter",
            "sessions_status_filter",
            "sessions_results_list"
        ]
    )

    public static let keyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "focus_search_shortcut",
            key: "f",
            modifiers: ["command"],
            actionSummary: "Focus the sessions search input."
        ),
        KeyboardShortcutDescriptor(
            id: "clear_filters_shortcut",
            key: "backspace",
            modifiers: ["command", "option"],
            actionSummary: "Clear active session filters."
        ),
    ]

    private let sessionLibrary: SessionLibraryService
    private let pendingTranscriptionService: (any PendingSessionTranscribing)?
    private let pendingNotificationService: any PendingSessionNotificationDetecting
    private var lastLoadedItems: [SessionSummaryDTO] = []
    public private(set) var pendingNotifications: [PendingSessionNotificationIntent] = []

    public init(
        sessionLibrary: SessionLibraryService,
        pendingTranscriptionService: (any PendingSessionTranscribing)? = nil,
        pendingNotificationService: any PendingSessionNotificationDetecting = PendingSessionNotificationService()
    ) {
        self.sessionLibrary = sessionLibrary
        self.pendingTranscriptionService = pendingTranscriptionService
        self.pendingNotificationService = pendingNotificationService
    }

    public func refresh() {
        state = .loading(previousItems: lastLoadedItems)

        do {
            let query = SessionQuery(
                status: filters.status.sessionStatus,
                mode: filters.mode.runtimeMode,
                searchText: normalizedSearchText(filters.searchText)
            )
            let listed = try sessionLibrary.listSessions(query: query)
            let ordered = listed.sorted(by: Self.deterministicNewestFirst)
            let transitionNotifications = pendingNotificationService.detectTransitionNotifications(
                previous: lastLoadedItems,
                current: ordered
            )
            if !transitionNotifications.isEmpty {
                pendingNotifications.append(contentsOf: transitionNotifications)
            }
            lastLoadedItems = ordered

            if ordered.isEmpty {
                state = .empty(Self.emptyState(for: filters))
            } else {
                state = .loaded(items: ordered)
            }
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError, recoverableItems: lastLoadedItems)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not load sessions right now.",
                    remediation: "Retry refresh. You can still open sessions already shown.",
                    debugDetail: String(describing: error)
                ),
                recoverableItems: lastLoadedItems
            )
        }
    }

    public func setModeFilter(_ mode: ModeFilter) {
        guard filters.mode != mode else { return }
        filters.mode = mode
        refresh()
    }

    public func setStatusFilter(_ status: StatusFilter) {
        guard filters.status != status else { return }
        filters.status = status
        refresh()
    }

    public func setSearchText(_ searchText: String) {
        if filters.searchText == searchText {
            return
        }
        filters.searchText = searchText
        refresh()
    }

    public func clearFilters() {
        filters = Filters()
        refresh()
    }

    public func transcribePendingSession(sessionID: String) async {
        guard let pendingTranscriptionService else {
            state = .failed(
                AppServiceError(
                    code: .runtimeUnavailable,
                    userMessage: "Pending-session transcription is unavailable.",
                    remediation: "Restart the app or check runtime service wiring."
                ),
                recoverableItems: lastLoadedItems
            )
            return
        }

        guard let summary = lastLoadedItems.first(where: { $0.sessionID == sessionID }) else {
            state = .failed(
                AppServiceError(
                    code: .invalidInput,
                    userMessage: "Selected session is no longer available.",
                    remediation: "Refresh sessions and retry."
                ),
                recoverableItems: lastLoadedItems
            )
            return
        }

        guard summary.readyToTranscribe, summary.pendingTranscriptionState == .readyToTranscribe else {
            state = .failed(
                AppServiceError(
                    code: .invalidInput,
                    userMessage: "Session is not ready to transcribe yet.",
                    remediation: "Wait until model setup is complete, then retry."
                ),
                recoverableItems: lastLoadedItems
            )
            return
        }

        state = .loading(previousItems: lastLoadedItems)
        do {
            _ = try await pendingTranscriptionService.transcribePendingSession(
                summary: summary,
                timeoutSeconds: 120
            )
            refresh()
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError, recoverableItems: lastLoadedItems)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not transcribe the pending session.",
                    remediation: "Retry the action after checking runtime health.",
                    debugDetail: String(describing: error)
                ),
                recoverableItems: lastLoadedItems
            )
        }
    }

    public var visibleItems: [SessionSummaryDTO] {
        switch state {
        case .loaded(let items):
            return items
        case .failed(_, let recoverableItems):
            return recoverableItems
        case .loading(let previousItems):
            return previousItems
        case .idle, .empty:
            return []
        }
    }

    public func consumePendingNotifications() -> [PendingSessionNotificationIntent] {
        let queued = pendingNotifications
        pendingNotifications.removeAll(keepingCapacity: false)
        return queued
    }

    private func normalizedSearchText(_ text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func deterministicNewestFirst(lhs: SessionSummaryDTO, rhs: SessionSummaryDTO) -> Bool {
        if lhs.startedAt != rhs.startedAt {
            return lhs.startedAt > rhs.startedAt
        }
        if lhs.sessionID != rhs.sessionID {
            return lhs.sessionID < rhs.sessionID
        }
        return lhs.rootPath.path < rhs.rootPath.path
    }

    private static func emptyState(for filters: Filters) -> EmptyState {
        let searchText = filters.searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        if !searchText.isEmpty {
            return EmptyState(
                title: "No sessions match your search.",
                detail: "Try different keywords, or clear filters to see all sessions."
            )
        }
        if filters.mode != .all || filters.status != .all {
            return EmptyState(
                title: "No sessions match these filters.",
                detail: "Adjust mode/status filters to broaden results."
            )
        }
        return EmptyState(
            title: "No sessions yet.",
            detail: "Start a live or record-only session to populate this list."
        )
    }
}
