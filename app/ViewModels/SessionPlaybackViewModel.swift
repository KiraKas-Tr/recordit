import Foundation

@MainActor
public final class SessionPlaybackViewModel {
    public enum ViewState: Equatable {
        case idle
        case unavailable(reason: String)
        case ready(AudioPlaybackSnapshot)
        case playing(AudioPlaybackSnapshot)
        case paused(AudioPlaybackSnapshot)
        case completed(AudioPlaybackSnapshot)
        case failed(AppServiceError)
    }

    public private(set) var state: ViewState = .idle

    public static let accessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "play_audio",
            label: "Play session audio",
            hint: "Starts playback from the current position."
        ),
        AccessibilityElementDescriptor(
            id: "pause_audio",
            label: "Pause session audio",
            hint: "Pauses playback at the current position."
        ),
        AccessibilityElementDescriptor(
            id: "seek_audio",
            label: "Seek audio position",
            hint: "Move forward or backward in the session audio."
        ),
    ]

    public static let focusPlan = KeyboardFocusPlan(
        orderedElementIDs: ["play_audio", "pause_audio", "seek_audio"]
    )

    public static let keyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "play_pause_shortcut",
            key: "space",
            modifiers: [],
            actionSummary: "Toggle play or pause."
        ),
        KeyboardShortcutDescriptor(
            id: "seek_forward_shortcut",
            key: "right",
            modifiers: ["option"],
            actionSummary: "Seek forward in audio."
        ),
        KeyboardShortcutDescriptor(
            id: "seek_backward_shortcut",
            key: "left",
            modifiers: ["option"],
            actionSummary: "Seek backward in audio."
        ),
    ]

    private let playbackService: SessionAudioPlaybackService

    public init(playbackService: SessionAudioPlaybackService) {
        self.playbackService = playbackService
    }

    public convenience init() {
        self.init(playbackService: SessionAudioPlaybackService())
    }

    public func load(sessionWavPath: URL?) {
        guard let sessionWavPath else {
            state = .unavailable(reason: "No audio artifact exists for this session.")
            return
        }

        do {
            let snapshot = try playbackService.loadAudio(at: sessionWavPath)
            state = .ready(snapshot)
        } catch let serviceError as AppServiceError {
            if serviceError.code == .artifactMissing {
                state = .unavailable(reason: "Audio playback is unavailable for this session.")
            } else {
                state = .failed(serviceError)
            }
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Audio playback setup failed.",
                    remediation: "Retry after reopening the session.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func play() {
        do {
            let snapshot = try playbackService.play()
            state = .playing(snapshot)
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not start playback.",
                    remediation: "Retry play or reopen session detail.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func pause() {
        do {
            let snapshot = try playbackService.pause()
            state = .paused(snapshot)
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not pause playback.",
                    remediation: "Retry pause.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func seek(normalizedProgress: Double) {
        let clamped = min(max(normalizedProgress, 0), 1)
        let snapshot = playbackService.refresh()
        let target = snapshot.durationSeconds * clamped

        do {
            let next = try playbackService.seek(to: target)
            applySnapshot(next)
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Could not seek playback.",
                    remediation: "Retry seeking.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public func refresh() {
        applySnapshot(playbackService.refresh())
    }

    private func applySnapshot(_ snapshot: AudioPlaybackSnapshot) {
        switch snapshot.phase {
        case .idle:
            state = .idle
        case .ready:
            state = .ready(snapshot)
        case .playing:
            state = .playing(snapshot)
        case .paused:
            state = .paused(snapshot)
        case .completed:
            state = .completed(snapshot)
        }
    }
}
