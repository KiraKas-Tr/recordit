import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("session_export_view_model_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private final class RecordingExportService: SessionExportService {
    var queuedResults: [Result<SessionExportResult, Error>]
    private(set) var requests: [SessionExportRequest] = []

    init(queuedResults: [Result<SessionExportResult, Error>]) {
        self.queuedResults = queuedResults
    }

    func exportSession(_ request: SessionExportRequest) throws -> SessionExportResult {
        requests.append(request)
        guard !queuedResults.isEmpty else {
            throw AppServiceError(
                code: .unknown,
                userMessage: "No queued export result.",
                remediation: "Add a fixture result before running smoke."
            )
        }
        return try queuedResults.removeFirst().get()
    }
}

private func result(kind: SessionExportKind, redacted: Bool) -> SessionExportResult {
    SessionExportResult(
        kind: kind,
        outputURL: URL(fileURLWithPath: "/tmp/output"),
        exportedAt: Date(),
        includedArtifacts: [],
        redacted: redacted
    )
}

@MainActor
private func runSmoke() {
    let service = RecordingExportService(
        queuedResults: [
            .success(result(kind: .diagnostics, redacted: false)),
            .failure(
                AppServiceError(
                    code: .permissionDenied,
                    userMessage: "Destination is not writable.",
                    remediation: "Choose a writable folder."
                )
            ),
        ]
    )
    let vm = SessionExportViewModel(exportService: service)

    check(vm.filenamePreview(sessionID: "sess 1") == "recordit-transcript-sess-1.txt", "unexpected transcript filename preview")
    vm.setExportKind(.audio)
    check(vm.filenamePreview(sessionID: "sess 1") == "recordit-audio-sess-1.wav", "unexpected audio filename preview")
    vm.setExportKind(.bundle)
    check(vm.filenamePreview(sessionID: "sess 1") == "recordit-session-sess-1.zip", "unexpected bundle filename preview")
    vm.setExportKind(.diagnostics)
    check(vm.filenamePreview(sessionID: "sess 1") == "recordit-diagnostics-sess-1.zip", "unexpected diagnostics filename preview")
    check(vm.diagnosticsOptionsVisible, "diagnostics options should be visible for diagnostics kind")

    vm.setDiagnosticsIncludeTranscriptText(true)
    vm.setDiagnosticsIncludeAudio(true)
    vm.runExport(
        sessionID: "sess 1",
        sessionRoot: URL(fileURLWithPath: "/tmp/session"),
        outputDirectory: URL(fileURLWithPath: "/tmp/exports")
    )
    guard let diagnosticsRequest = service.requests.first else {
        check(false, "expected diagnostics export request")
        return
    }
    check(diagnosticsRequest.kind == .diagnostics, "expected diagnostics request kind")
    check(diagnosticsRequest.includeTranscriptTextInDiagnostics, "diagnostics request should include transcript opt-in")
    check(diagnosticsRequest.includeAudioInDiagnostics, "diagnostics request should include audio opt-in")
    check(vm.completionMessage == "Diagnostics exported with transcript text.", "expected diagnostics success message")

    vm.setExportKind(.transcript)
    check(!vm.diagnosticsOptionsVisible, "diagnostics options should be hidden outside diagnostics kind")
    check(!vm.includeTranscriptTextInDiagnostics, "transcript opt-in should reset when leaving diagnostics kind")
    check(!vm.includeAudioInDiagnostics, "audio opt-in should reset when leaving diagnostics kind")

    vm.runExport(
        sessionID: "sess 1",
        sessionRoot: URL(fileURLWithPath: "/tmp/session"),
        outputDirectory: URL(fileURLWithPath: "/tmp/exports")
    )
    guard case let .failed(error) = vm.state else {
        check(false, "second export should fail")
        return
    }
    check(error.code == .permissionDenied, "error code should preserve service error")
    check(vm.errorMessage == "Destination is not writable.", "error message should surface service user message")
}

@main
struct SessionExportViewModelSmokeMain {
    static func main() {
        runSmoke()
        print("session_export_view_model_smoke: PASS")
    }
}
