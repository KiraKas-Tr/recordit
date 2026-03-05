import Foundation

enum PendingTransitionSmokeError: Error {
    case expectationFailed(String)
}

@main
struct PendingTransitionSmoke {
    static func main() throws {
        let transitionService = PendingSessionTransitionService()

        var state: PendingTranscriptionState = .pendingModel
        state = try transitionService.transition(from: state, event: .modelAvailable)
        state = try transitionService.transition(from: state, event: .transcriptionStarted)
        state = try transitionService.transition(from: state, event: .transcriptionCompleted)
        guard state == .completed else {
            throw PendingTransitionSmokeError.expectationFailed("expected completed state")
        }

        var illegalRejected = false
        do {
            _ = try transitionService.transition(from: .pendingModel, event: .transcriptionStarted)
        } catch {
            illegalRejected = true
        }
        guard illegalRejected else {
            throw PendingTransitionSmokeError.expectationFailed("illegal transition must be rejected")
        }

        let sidecarService = FileSystemPendingSessionSidecarService()
        let tempRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent("pending-transition-smoke-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempRoot, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let sessionRoot = tempRoot
            .appendingPathComponent("20260305", isDirectory: true)
            .appendingPathComponent("20260305T000000Z-record_only", isDirectory: true)
        try FileManager.default.createDirectory(at: sessionRoot, withIntermediateDirectories: true)

        let wavURL = sessionRoot.appendingPathComponent("session.wav")
        try Data("wav".utf8).write(to: wavURL)

        let writeRequest = PendingSessionSidecarWriteRequest(
            sessionID: "session-a",
            sessionRoot: sessionRoot,
            wavPath: wavURL,
            createdAt: Date(timeIntervalSince1970: 1_700_000_000),
            mode: .recordOnly,
            transcriptionState: .pendingModel
        )
        _ = try sidecarService.writePendingSidecar(writeRequest)

        let serviceReady = FileSystemSessionLibraryService(
            sessionsRootProvider: { tempRoot },
            pendingSidecarService: sidecarService,
            pendingTransitionService: transitionService,
            modelAvailabilityProvider: { true }
        )
        let readySessions = try serviceReady.listSessions(query: SessionQuery())
        guard readySessions.count == 1 else {
            throw PendingTransitionSmokeError.expectationFailed("expected one discovered session")
        }
        guard readySessions[0].pendingTranscriptionState == .readyToTranscribe else {
            throw PendingTransitionSmokeError.expectationFailed("expected transition to ready_to_transcribe")
        }
        guard readySessions[0].readyToTranscribe else {
            throw PendingTransitionSmokeError.expectationFailed("readyToTranscribe must be true")
        }

        let persistedReady = try sidecarService.loadPendingSidecar(
            at: sessionRoot.appendingPathComponent("session.pending.json")
        )
        guard persistedReady.transcriptionState == .readyToTranscribe else {
            throw PendingTransitionSmokeError.expectationFailed(
                "expected persisted transition to ready_to_transcribe"
            )
        }

        let servicePending = FileSystemSessionLibraryService(
            sessionsRootProvider: { tempRoot },
            pendingSidecarService: sidecarService,
            pendingTransitionService: transitionService,
            modelAvailabilityProvider: { false }
        )
        let pendingSessions = try servicePending.listSessions(query: SessionQuery())
        guard pendingSessions[0].pendingTranscriptionState == .pendingModel else {
            throw PendingTransitionSmokeError.expectationFailed("expected transition back to pending_model")
        }

        print("pending_transition_smoke: PASS")
    }
}
