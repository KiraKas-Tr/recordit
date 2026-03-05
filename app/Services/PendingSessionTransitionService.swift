import Foundation

public enum PendingSessionTransitionEvent: String, Codable, Sendable {
    case modelAvailable = "model_available"
    case modelUnavailable = "model_unavailable"
    case transcriptionStarted = "transcription_started"
    case transcriptionCompleted = "transcription_completed"
    case transcriptionFailed = "transcription_failed"
}

public struct PendingSessionTransitionService: Sendable {
    public init() {}

    public func transition(
        from current: PendingTranscriptionState,
        event: PendingSessionTransitionEvent
    ) throws -> PendingTranscriptionState {
        switch (current, event) {
        case (.pendingModel, .modelAvailable):
            return .readyToTranscribe
        case (.readyToTranscribe, .modelUnavailable):
            return .pendingModel
        case (.readyToTranscribe, .transcriptionStarted):
            return .transcribing
        case (.transcribing, .transcriptionCompleted):
            return .completed
        case (.transcribing, .transcriptionFailed):
            return .failed
        case (.failed, .modelAvailable):
            return .readyToTranscribe
        default:
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "Pending-session transition is invalid.",
                remediation: "Retry from a valid transition point or rebuild pending metadata.",
                debugDetail: "illegal_transition current=\(current.rawValue) event=\(event.rawValue)"
            )
        }
    }

    public func reconcileReadiness(
        current: PendingTranscriptionState,
        modelAvailable: Bool
    ) throws -> PendingTranscriptionState {
        switch current {
        case .pendingModel where modelAvailable:
            return try transition(from: current, event: .modelAvailable)
        case .readyToTranscribe where !modelAvailable:
            return try transition(from: current, event: .modelUnavailable)
        default:
            return current
        }
    }

    public func isReadyToTranscribe(_ state: PendingTranscriptionState) -> Bool {
        state == .readyToTranscribe
    }
}
