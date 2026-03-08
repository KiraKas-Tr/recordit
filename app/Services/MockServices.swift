import Foundation

public actor MockRuntimeService: RuntimeService {
    public private(set) var launches: [RuntimeStartRequest] = []
    public private(set) var controls: [(pid: Int32, action: RuntimeControlAction)] = []

    public init() {}

    public func startSession(request: RuntimeStartRequest) async throws -> RuntimeLaunchResult {
        launches.append(request)
        return RuntimeLaunchResult(processIdentifier: 42, sessionRoot: request.outputRoot, startedAt: Date())
    }

    public func controlSession(processIdentifier: Int32, action: RuntimeControlAction) async throws -> RuntimeControlResult {
        controls.append((pid: processIdentifier, action: action))
        return RuntimeControlResult(accepted: true, detail: "mocked")
    }
}

public struct MockJsonlTailService: JsonlTailService {
    public var queuedEvents: [RuntimeEventDTO]

    public init(queuedEvents: [RuntimeEventDTO] = []) {
        self.queuedEvents = queuedEvents
    }

    public func readEvents(at jsonlPath: URL, from cursor: JsonlTailCursor) throws -> ([RuntimeEventDTO], JsonlTailCursor) {
        let nextCursor = JsonlTailCursor(
            byteOffset: cursor.byteOffset + UInt64(queuedEvents.count * 32),
            lineCount: cursor.lineCount + UInt64(queuedEvents.count),
            lastModifiedAt: Date()
        )
        return (queuedEvents, nextCursor)
    }
}

public struct MockManifestService: ManifestService {
    public var manifest: SessionManifestDTO

    public init(manifest: SessionManifestDTO) {
        self.manifest = manifest
    }

    public func loadManifest(at manifestPath: URL) throws -> SessionManifestDTO {
        manifest
    }
}

public struct MockModelResolutionService: ModelResolutionService {
    public var resolution: ResolvedModelDTO

    public init(resolution: ResolvedModelDTO) {
        self.resolution = resolution
    }

    public func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        resolution
    }
}

public struct MockPendingSessionSidecarService: PendingSessionSidecarService {
    public var sidecarByPath: [String: PendingSessionSidecarDTO]

    public init(sidecarByPath: [String: PendingSessionSidecarDTO] = [:]) {
        self.sidecarByPath = sidecarByPath
    }

    public func writePendingSidecar(_ request: PendingSessionSidecarWriteRequest) throws -> PendingSessionSidecarDTO {
        let sessionID = request.sessionID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sessionID.isEmpty else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Pending session metadata is incomplete.",
                remediation: "Provide a non-empty session ID for record-only sessions."
            )
        }
        guard request.mode == .recordOnly else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Pending sidecar mode is invalid.",
                remediation: "Use `record_only` mode for pending sidecars."
            )
        }

        return PendingSessionSidecarDTO(
            sessionID: sessionID,
            createdAtUTC: ISO8601DateFormatter().string(from: request.createdAt),
            wavPath: request.wavPath.standardizedFileURL.path,
            mode: .recordOnly,
            transcriptionState: request.transcriptionState
        )
    }

    public func loadPendingSidecar(at pendingSidecarPath: URL) throws -> PendingSessionSidecarDTO {
        let path = pendingSidecarPath.standardizedFileURL.path
        guard let sidecar = sidecarByPath[path] else {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "Pending sidecar was not found.",
                remediation: "Write pending metadata before loading it.",
                debugDetail: path
            )
        }
        return sidecar
    }
}

public struct MockArtifactIntegrityService: ArtifactIntegrityService {
    public var report: SessionArtifactIntegrityReportDTO

    public init(report: SessionArtifactIntegrityReportDTO) {
        self.report = report
    }

    public func evaluateSessionArtifacts(
        sessionID: String,
        rootPath: URL
    ) throws -> SessionArtifactIntegrityReportDTO {
        var resolved = report
        if resolved.sessionID.isEmpty {
            resolved.sessionID = sessionID
        }
        if resolved.rootPath.path.isEmpty {
            resolved.rootPath = rootPath.standardizedFileURL
        }
        return resolved
    }
}

public struct MockSessionLibraryService: SessionLibraryService {
    public var sessions: [SessionSummaryDTO]
    public var deletedSessionIDs: Set<String>
    public var trashedRootsBySessionID: [String: URL]

    public init(
        sessions: [SessionSummaryDTO] = [],
        deletedSessionIDs: Set<String> = [],
        trashedRootsBySessionID: [String: URL] = [:]
    ) {
        self.sessions = sessions
        self.deletedSessionIDs = deletedSessionIDs
        self.trashedRootsBySessionID = trashedRootsBySessionID
    }

    public func listSessions(query: SessionQuery) throws -> [SessionSummaryDTO] {
        sessions.filter { item in
            guard !deletedSessionIDs.contains(item.sessionID) else {
                return false
            }
            let statusMatch = query.status.map { $0 == item.status } ?? true
            let modeMatch = query.mode.map { $0 == item.mode } ?? true
            let textMatch = query.searchText.map { text in
                item.sessionID.localizedCaseInsensitiveContains(text)
            } ?? true
            return statusMatch && modeMatch && textMatch
        }
    }

    public func deleteSession(
        sessionID: String,
        rootPath: URL,
        confirmTrash: Bool
    ) throws -> SessionDeletionResultDTO {
        guard confirmTrash else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Session deletion requires confirmation.",
                remediation: "Confirm deletion before deleting the session."
            )
        }

        let resolvedSessionID = sessionID.isEmpty ? rootPath.lastPathComponent : sessionID
        let trashedRoot = trashedRootsBySessionID[resolvedSessionID]
        return SessionDeletionResultDTO(
            sessionID: resolvedSessionID,
            originalRootPath: rootPath.standardizedFileURL,
            trashedRootPath: trashedRoot?.standardizedFileURL,
            didMoveToTrash: true
        )
    }
}
