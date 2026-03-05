import Foundation

private final class CaptureArchiveBuilder {
    private(set) var snapshotDirectories: [URL] = []

    var lastSnapshotDirectory: URL? {
        snapshotDirectories.last
    }

    func build(sourceDirectory: URL, destinationZip: URL) throws {
        let fm = FileManager.default
        let snapshot = fm.temporaryDirectory
            .appendingPathComponent("recordit-export-snapshot-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: snapshot, withIntermediateDirectories: true, attributes: nil)
        let destinationSnapshot = snapshot.appendingPathComponent(sourceDirectory.lastPathComponent, isDirectory: true)
        try fm.copyItem(at: sourceDirectory, to: destinationSnapshot)
        snapshotDirectories.append(destinationSnapshot)
        try Data("zip-placeholder".utf8).write(to: destinationZip, options: .atomic)
    }
}

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("export_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func readTextFile(_ url: URL) throws -> String {
    let handle = try FileHandle(forReadingFrom: url)
    defer { try? handle.close() }
    let data = try handle.readToEnd() ?? Data()
    return String(decoding: data, as: UTF8.self)
}

private func fixtureManifestData() -> Data {
    let payload: [String: Any] = [
        "schema_version": "1",
        "kind": "transcribe-live-runtime",
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "transcript": "top-level transcript fallback",
        "terminal_summary": [
            "stable_lines": [
                "[00:00.000-00:00.200] mic: hello",
                "[00:00.300-00:00.800] system: hi"
            ]
        ]
    ]
    do {
        return try JSONSerialization.data(withJSONObject: payload, options: [.prettyPrinted, .sortedKeys])
    } catch {
        fputs("export_smoke failed: could not encode manifest fixture: \(error)\n", stderr)
        exit(1)
    }
}

private func fixtureJsonlText() -> String {
    [
        "{\"event_type\":\"final\",\"text\":\"alpha\"}",
        "{\"event_type\":\"llm_final\",\"text\":\"beta\"}",
        "{\"event_type\":\"reconciled_final\",\"text\":\"gamma\"}",
        "{\"event_type\":\"partial\",\"text\":\"draft\"}"
    ].joined(separator: "\n")
}

private func createFixtureSession(at root: URL) throws {
    let fm = FileManager.default
    try fm.createDirectory(at: root, withIntermediateDirectories: true, attributes: nil)

    try fixtureManifestData().write(
        to: root.appendingPathComponent("session.manifest.json"),
        options: .atomic
    )
    try Data(fixtureJsonlText().utf8).write(
        to: root.appendingPathComponent("session.jsonl"),
        options: .atomic
    )
    try Data([0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00]).write(
        to: root.appendingPathComponent("session.wav"),
        options: .atomic
    )
}

private func runSmoke() {
    let fm = FileManager.default
    let tempRoot = fm.temporaryDirectory
        .appendingPathComponent("recordit-export-smoke-\(UUID().uuidString)", isDirectory: true)
    defer { try? fm.removeItem(at: tempRoot) }

    do {
        try fm.createDirectory(at: tempRoot, withIntermediateDirectories: true, attributes: nil)
    } catch {
        check(false, "could not create temp root: \(error)")
        return
    }

    let dataRoot = tempRoot.appendingPathComponent("container-data", isDirectory: true)
    let sessionsRoot = dataRoot
        .appendingPathComponent("artifacts", isDirectory: true)
        .appendingPathComponent("packaged-beta", isDirectory: true)
        .appendingPathComponent("sessions", isDirectory: true)
    let sessionRoot = sessionsRoot.appendingPathComponent("20260305T000000Z-live", isDirectory: true)
    let exportDirectory = sessionsRoot.appendingPathComponent("exports", isDirectory: true)

    do {
        try createFixtureSession(at: sessionRoot)
        try fm.createDirectory(at: exportDirectory, withIntermediateDirectories: true, attributes: nil)
    } catch {
        check(false, "fixture setup failed: \(error)")
        return
    }

    let archiveCapture = CaptureArchiveBuilder()
    let service = FileSystemSessionExportService(
        archiveBuilder: archiveCapture.build,
        environment: [
            "RECORDIT_ENFORCE_APP_MANAGED_STORAGE_POLICY": "1",
            "RECORDIT_CONTAINER_DATA_ROOT": dataRoot.path
        ]
    )

    do {
        let transcriptResult = try service.exportSession(
            SessionExportRequest(
                sessionID: "sess-1",
                sessionRoot: sessionRoot,
                outputDirectory: exportDirectory,
                kind: .transcript
            )
        )
        check(transcriptResult.outputURL.lastPathComponent == "recordit-transcript-sess-1.txt", "unexpected transcript filename")
        let transcript = try readTextFile(transcriptResult.outputURL)
        check(transcript.contains("mic: hello"), "transcript export should prefer manifest stable lines")

        let audioResult = try service.exportSession(
            SessionExportRequest(
                sessionID: "sess-1",
                sessionRoot: sessionRoot,
                outputDirectory: exportDirectory,
                kind: .audio
            )
        )
        check(audioResult.outputURL.lastPathComponent == "recordit-audio-sess-1.wav", "unexpected audio filename")
        check(fm.fileExists(atPath: audioResult.outputURL.path), "audio export file missing")

        let bundleResult = try service.exportSession(
            SessionExportRequest(
                sessionID: "sess-1",
                sessionRoot: sessionRoot,
                outputDirectory: exportDirectory,
                kind: .bundle
            )
        )
        check(bundleResult.outputURL.lastPathComponent == "recordit-session-sess-1.zip", "unexpected bundle filename")
        check(bundleResult.includedArtifacts.contains("session.manifest.json"), "bundle should include manifest")

        let diagnosticsResult = try service.exportSession(
            SessionExportRequest(
                sessionID: "sess-1",
                sessionRoot: sessionRoot,
                outputDirectory: exportDirectory,
                kind: .diagnostics,
                includeTranscriptTextInDiagnostics: false,
                includeAudioInDiagnostics: false
            )
        )
        check(diagnosticsResult.outputURL.lastPathComponent == "recordit-diagnostics-sess-1.zip", "unexpected diagnostics filename")
        check(diagnosticsResult.redacted, "diagnostics default should be redacted")

        guard let redactedSnapshot = archiveCapture.lastSnapshotDirectory else {
            check(false, "archive builder snapshot missing")
            return
        }
        let redactedManifest = try readTextFile(redactedSnapshot.appendingPathComponent("session.manifest.json"))
        check(redactedManifest.contains("[REDACTED]"), "redacted diagnostics manifest should scrub transcript text")
        let redactedJsonl = try readTextFile(redactedSnapshot.appendingPathComponent("session.jsonl"))
        check(redactedJsonl.contains("\"text\":\"[REDACTED]\""), "redacted diagnostics jsonl should scrub text fields")
        check(!redactedJsonl.contains("\"text\":\"gamma\""), "redacted diagnostics jsonl should remove original transcript text")

        let redactedDiagnosticsMetadata = try readTextFile(
            redactedSnapshot.appendingPathComponent("diagnostics.json")
        )
        check(
            redactedDiagnosticsMetadata.contains("\"include_transcript_text\" : false"),
            "default diagnostics metadata should keep transcript opt-in disabled"
        )

        let diagnosticsOptInResult = try service.exportSession(
            SessionExportRequest(
                sessionID: "sess-1",
                sessionRoot: sessionRoot,
                outputDirectory: exportDirectory,
                kind: .diagnostics,
                includeTranscriptTextInDiagnostics: true,
                includeAudioInDiagnostics: false
            )
        )
        check(!diagnosticsOptInResult.redacted, "diagnostics opt-in should include transcript text")
        guard let optInSnapshot = archiveCapture.lastSnapshotDirectory else {
            check(false, "archive builder snapshot missing for diagnostics opt-in export")
            return
        }
        let optInManifest = try readTextFile(optInSnapshot.appendingPathComponent("session.manifest.json"))
        check(
            optInManifest.contains("[00:00.000-00:00.200] mic: hello"),
            "diagnostics opt-in should preserve manifest transcript text"
        )
        let optInJsonl = try readTextFile(optInSnapshot.appendingPathComponent("session.jsonl"))
        check(optInJsonl.contains("\"text\":\"gamma\""), "diagnostics opt-in should preserve jsonl transcript text")
        let optInDiagnosticsMetadata = try readTextFile(
            optInSnapshot.appendingPathComponent("diagnostics.json")
        )
        check(
            optInDiagnosticsMetadata.contains("\"include_transcript_text\" : true"),
            "opt-in diagnostics metadata should record transcript inclusion"
        )

        let outsideDestination = tempRoot.appendingPathComponent("outside", isDirectory: true)
        try fm.createDirectory(at: outsideDestination, withIntermediateDirectories: true, attributes: nil)
        do {
            _ = try service.exportSession(
                SessionExportRequest(
                    sessionID: "sess-1",
                    sessionRoot: sessionRoot,
                    outputDirectory: outsideDestination,
                    kind: .transcript
                )
            )
            check(false, "policy should reject export outside managed sessions root")
        } catch let error as AppServiceError {
            check(error.code == .permissionDenied, "expected permissionDenied for policy violation")
        }
    } catch {
        check(false, "smoke run failed: \(error)")
    }
}

@main
struct ExportSmokeMain {
    static func main() {
        runSmoke()
        print("export_smoke: PASS")
    }
}
