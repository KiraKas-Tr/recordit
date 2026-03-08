import Foundation

public struct ManifestFinalStatusMapper {
    public init() {}

    public func mapStatus(_ manifest: SessionManifestDTO) -> SessionStatus {
        let normalized = manifest.status
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        switch normalized {
        case SessionStatus.failed.rawValue:
            return .failed
        case SessionStatus.degraded.rawValue:
            return .degraded
        case SessionStatus.pending.rawValue:
            return .pending
        case SessionStatus.ok.rawValue:
            return manifest.trustNoticeCount > 0 ? .degraded : .ok
        default:
            return manifest.trustNoticeCount > 0 ? .degraded : .ok
        }
    }
}
