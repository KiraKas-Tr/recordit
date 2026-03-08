import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("legacy_flat_migration_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func writeJSON(_ object: Any, to url: URL) throws {
    let data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: url, options: .atomic)
}

private func writeData(_ data: Data, to url: URL) throws {
    try data.write(to: url, options: .atomic)
}

private func readData(_ url: URL) throws -> Data {
    let handle = try FileHandle(forReadingFrom: url)
    defer { try? handle.close() }
    return try handle.readToEnd() ?? Data()
}

private func createCanonicalSession(at root: URL, sessionID: String) throws {
    let fm = FileManager.default
    try fm.createDirectory(at: root, withIntermediateDirectories: true, attributes: nil)

    let manifest: [String: Any] = [
        "session_id": sessionID,
        "generated_at_utc": "2026-03-05T00:02:00Z",
        "runtime_mode": "live",
        "session_summary": [
            "session_status": "ok",
            "duration_sec": 90
        ]
    ]
    try writeJSON(manifest, to: root.appendingPathComponent("session.manifest.json"))
    try writeData(Data([0x52, 0x49, 0x46, 0x46, 0x00]), to: root.appendingPathComponent("session.wav"))
}

private func createLegacyFlatSet(
    under root: URL,
    stem: String,
    sessionID: String,
    transcriptText: String
) throws -> (manifest: URL, wav: URL, jsonl: URL) {
    let manifestURL = root.appendingPathComponent("\(stem).manifest.json")
    let wavURL = root.appendingPathComponent("\(stem).wav")
    let jsonlURL = root.appendingPathComponent("\(stem).jsonl")

    let manifest: [String: Any] = [
        "session_id": sessionID,
        "generated_at_utc": "2026-03-05T00:01:00Z",
        "runtime_mode": "record_only",
        "session_summary": [
            "session_status": "pending",
            "duration_sec": 0
        ]
    ]
    try writeJSON(manifest, to: manifestURL)
    try writeData(Data([0x52, 0x49, 0x46, 0x46, 0x10]), to: wavURL)
    try writeData(Data("{\"event_type\":\"final\",\"text\":\"\(transcriptText)\"}\n".utf8), to: jsonlURL)
    return (manifestURL, wavURL, jsonlURL)
}

private func runSmoke() throws {
    let fm = FileManager.default
    let tempRoot = fm.temporaryDirectory
        .appendingPathComponent("recordit-legacy-flat-migration-\(UUID().uuidString)", isDirectory: true)
    defer { try? fm.removeItem(at: tempRoot) }
    try fm.createDirectory(at: tempRoot, withIntermediateDirectories: true, attributes: nil)

    let canonicalStem = "20260305T000200Z-live"
    let canonicalRoot = tempRoot.appendingPathComponent(canonicalStem, isDirectory: true)
    try createCanonicalSession(at: canonicalRoot, sessionID: "canonical-session")

    _ = try createLegacyFlatSet(
        under: tempRoot,
        stem: canonicalStem,
        sessionID: "duplicate-flat-session",
        transcriptText: "duplicate row"
    )
    let importedStem = "20260305T000100Z-record-only"
    let imported = try createLegacyFlatSet(
        under: tempRoot,
        stem: importedStem,
        sessionID: "legacy-flat-session",
        transcriptText: "legacy row"
    )

    let importedManifestBefore = try readData(imported.manifest)
    let importedWavBefore = try readData(imported.wav)
    let importedJsonlBefore = try readData(imported.jsonl)

    let service = FileSystemSessionLibraryService(
        sessionsRootProvider: { tempRoot },
        modelAvailabilityProvider: { true }
    )
    let first = try service.listSessions(query: SessionQuery())
    check(first.count == 2, "expected canonical + one legacy import")
    check(first.contains(where: { $0.sessionID == "canonical-session" }), "missing canonical session")
    check(!first.contains(where: { $0.sessionID == "duplicate-flat-session" }), "duplicate flat import should be skipped")

    guard let legacy = first.first(where: { $0.ingestSource == .legacyFlatImport }) else {
        check(false, "expected one legacy flat import record")
        return
    }
    check(legacy.sessionID == "legacy-flat-session", "legacy import should keep manifest session_id")
    check(legacy.ingestDiagnostics["ingest_source"] == SessionIngestSource.legacyFlatImport.rawValue, "legacy diagnostics should report ingest source")
    check(legacy.ingestDiagnostics["legacy_manifest_path"] == imported.manifest.path, "legacy diagnostics should include manifest path")
    check(legacy.ingestDiagnostics["legacy_wav_path"] == imported.wav.path, "legacy diagnostics should include wav path")
    check(legacy.ingestDiagnostics["legacy_jsonl_path"] == imported.jsonl.path, "legacy diagnostics should include jsonl path")

    let second = try service.listSessions(query: SessionQuery())
    check(second.count == 2, "second scan should not duplicate imports")

    let importedManifestAfter = try readData(imported.manifest)
    let importedWavAfter = try readData(imported.wav)
    let importedJsonlAfter = try readData(imported.jsonl)
    check(importedManifestBefore == importedManifestAfter, "manifest source should not be rewritten")
    check(importedWavBefore == importedWavAfter, "wav source should not be rewritten")
    check(importedJsonlBefore == importedJsonlAfter, "jsonl source should not be rewritten")
}

@main
struct LegacyFlatMigrationSmokeMain {
    static func main() throws {
        try runSmoke()
        print("legacy_flat_migration_smoke: PASS")
    }
}
