import Foundation
#if canImport(AVFoundation)
import AVFoundation
#endif

public enum AudioPlaybackPhase: String, Codable, Sendable {
    case idle
    case ready
    case playing
    case paused
    case completed
}

public struct AudioPlaybackSnapshot: Equatable, Sendable {
    public var phase: AudioPlaybackPhase
    public var durationSeconds: TimeInterval
    public var currentTimeSeconds: TimeInterval

    public init(phase: AudioPlaybackPhase, durationSeconds: TimeInterval, currentTimeSeconds: TimeInterval) {
        self.phase = phase
        self.durationSeconds = durationSeconds
        self.currentTimeSeconds = currentTimeSeconds
    }

    public var progress: Double {
        guard durationSeconds > 0 else { return 0 }
        return min(max(currentTimeSeconds / durationSeconds, 0), 1)
    }
}

public protocol AudioPlayerEngine: AnyObject {
    var duration: TimeInterval { get }
    var currentTime: TimeInterval { get set }
    var isPlaying: Bool { get }

    func prepareToPlay() -> Bool
    func play() -> Bool
    func pause()
}

public protocol AudioPlayerEngineFactory {
    func make(url: URL) throws -> AudioPlayerEngine
}

#if canImport(AVFoundation)
private final class AVAudioPlayerEngine: NSObject, AudioPlayerEngine {
    private let player: AVAudioPlayer

    init(url: URL) throws {
        self.player = try AVAudioPlayer(contentsOf: url)
    }

    var duration: TimeInterval {
        player.duration
    }

    var currentTime: TimeInterval {
        get { player.currentTime }
        set { player.currentTime = newValue }
    }

    var isPlaying: Bool {
        player.isPlaying
    }

    func prepareToPlay() -> Bool {
        player.prepareToPlay()
    }

    func play() -> Bool {
        player.play()
    }

    func pause() {
        player.pause()
    }
}
#endif

public struct SystemAudioPlayerEngineFactory: AudioPlayerEngineFactory {
    public init() {}

    public func make(url: URL) throws -> AudioPlayerEngine {
        #if canImport(AVFoundation)
        return try AVAudioPlayerEngine(url: url)
        #else
        throw AppServiceError(
            code: .runtimeUnavailable,
            userMessage: "Audio playback is unavailable on this build target.",
            remediation: "Run this feature on a supported macOS runtime.",
            debugDetail: "AVFoundation unavailable"
        )
        #endif
    }
}

@MainActor
public final class SessionAudioPlaybackService {
    private let fileManager: FileManager
    private let engineFactory: AudioPlayerEngineFactory

    private var engine: AudioPlayerEngine?
    private var phase: AudioPlaybackPhase = .idle

    public init(
        fileManager: FileManager = .default,
        engineFactory: AudioPlayerEngineFactory = SystemAudioPlayerEngineFactory()
    ) {
        self.fileManager = fileManager
        self.engineFactory = engineFactory
    }

    public func loadAudio(at wavPath: URL) throws -> AudioPlaybackSnapshot {
        let path = wavPath.standardizedFileURL
        guard fileManager.fileExists(atPath: path.path) else {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "Session audio is unavailable.",
                remediation: "Record or transcribe this session again to generate `session.wav`.",
                debugDetail: path.path
            )
        }

        let createdEngine: AudioPlayerEngine
        do {
            createdEngine = try engineFactory.make(url: path)
        } catch let serviceError as AppServiceError {
            throw serviceError
        } catch {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Could not open session audio.",
                remediation: "Verify the audio file is readable and retry.",
                debugDetail: String(describing: error)
            )
        }

        guard createdEngine.prepareToPlay() else {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Session audio is not ready for playback.",
                remediation: "Retry playback. If this persists, re-export audio for the session.",
                debugDetail: path.path
            )
        }

        self.engine = createdEngine
        self.phase = .ready
        return snapshot()
    }

    public func play() throws -> AudioPlaybackSnapshot {
        guard let engine else {
            throw missingAudioNotLoadedError()
        }
        guard engine.play() else {
            throw AppServiceError(
                code: .ioFailure,
                userMessage: "Playback could not start.",
                remediation: "Retry playback or reopen the session detail.",
                debugDetail: "player.play() returned false"
            )
        }
        phase = .playing
        return snapshot()
    }

    public func pause() throws -> AudioPlaybackSnapshot {
        guard let engine else {
            throw missingAudioNotLoadedError()
        }
        engine.pause()
        phase = .paused
        return snapshot()
    }

    public func seek(to seconds: TimeInterval) throws -> AudioPlaybackSnapshot {
        guard let engine else {
            throw missingAudioNotLoadedError()
        }

        let clamped = min(max(seconds, 0), engine.duration)
        engine.currentTime = clamped

        if engine.isPlaying {
            phase = .playing
        } else if clamped >= engine.duration, engine.duration > 0 {
            phase = .completed
        } else if phase == .idle {
            phase = .ready
        }

        return snapshot()
    }

    public func refresh() -> AudioPlaybackSnapshot {
        snapshot()
    }

    private func snapshot() -> AudioPlaybackSnapshot {
        guard let engine else {
            return AudioPlaybackSnapshot(phase: .idle, durationSeconds: 0, currentTimeSeconds: 0)
        }

        if engine.isPlaying {
            phase = .playing
        } else if engine.duration > 0, engine.currentTime >= engine.duration {
            phase = .completed
        } else if phase == .playing {
            phase = .paused
        }

        return AudioPlaybackSnapshot(
            phase: phase,
            durationSeconds: engine.duration,
            currentTimeSeconds: engine.currentTime
        )
    }

    private func missingAudioNotLoadedError() -> AppServiceError {
        AppServiceError(
            code: .artifactMissing,
            userMessage: "No session audio is loaded.",
            remediation: "Load a session with a valid `session.wav` before using playback controls."
        )
    }
}
