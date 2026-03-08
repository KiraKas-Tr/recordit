import Foundation

@MainActor
public final class SessionExportViewModel {
    public enum FlowState: Equatable {
        case idle
        case exporting(SessionExportKind)
        case succeeded(SessionExportResult)
        case failed(AppServiceError)
    }

    public private(set) var state: FlowState = .idle
    public private(set) var selectedKind: SessionExportKind = .transcript
    public private(set) var includeTranscriptTextInDiagnostics = false
    public private(set) var includeAudioInDiagnostics = false

    private let exportService: any SessionExportService

    public init(exportService: any SessionExportService) {
        self.exportService = exportService
    }

    public func setExportKind(_ kind: SessionExportKind) {
        selectedKind = kind
        if kind != .diagnostics {
            includeTranscriptTextInDiagnostics = false
            includeAudioInDiagnostics = false
        }
    }

    public func setDiagnosticsIncludeTranscriptText(_ enabled: Bool) {
        includeTranscriptTextInDiagnostics = enabled
    }

    public func setDiagnosticsIncludeAudio(_ enabled: Bool) {
        includeAudioInDiagnostics = enabled
    }

    public var diagnosticsOptionsVisible: Bool {
        selectedKind == .diagnostics
    }

    public func filenamePreview(sessionID: String) -> String {
        let safeID = Self.safeSessionIdentifier(sessionID)
        switch selectedKind {
        case .transcript:
            return "recordit-transcript-\(safeID).txt"
        case .audio:
            return "recordit-audio-\(safeID).wav"
        case .bundle:
            return "recordit-session-\(safeID).zip"
        case .diagnostics:
            return "recordit-diagnostics-\(safeID).zip"
        }
    }

    public func runExport(
        sessionID: String,
        sessionRoot: URL,
        outputDirectory: URL
    ) {
        state = .exporting(selectedKind)

        let request = SessionExportRequest(
            sessionID: sessionID,
            sessionRoot: sessionRoot,
            outputDirectory: outputDirectory,
            kind: selectedKind,
            includeTranscriptTextInDiagnostics: selectedKind == .diagnostics ? includeTranscriptTextInDiagnostics : false,
            includeAudioInDiagnostics: selectedKind == .diagnostics ? includeAudioInDiagnostics : false
        )

        do {
            let result = try exportService.exportSession(request)
            state = .succeeded(result)
        } catch let error as AppServiceError {
            state = .failed(error)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Export failed.",
                    remediation: "Retry export after checking permissions and destination paths.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public var completionMessage: String? {
        guard case let .succeeded(result) = state else {
            return nil
        }

        switch result.kind {
        case .transcript:
            return "Transcript exported successfully."
        case .audio:
            return "Audio exported successfully."
        case .bundle:
            return "Session bundle exported successfully."
        case .diagnostics:
            if result.redacted {
                return "Diagnostics exported (transcript text redacted)."
            }
            return "Diagnostics exported with transcript text."
        }
    }

    public var errorMessage: String? {
        guard case let .failed(error) = state else {
            return nil
        }
        return error.userMessage
    }

    private static func safeSessionIdentifier(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return "session"
        }

        var value = ""
        value.reserveCapacity(trimmed.count)
        for scalar in trimmed.unicodeScalars {
            if CharacterSet.alphanumerics.contains(scalar) || scalar == "-" || scalar == "_" {
                value.unicodeScalars.append(scalar)
            } else {
                value.append("-")
            }
        }
        return value.isEmpty ? "session" : value
    }
}
