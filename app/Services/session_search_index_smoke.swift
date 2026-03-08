import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("session_search_index_smoke failed: \(message)\n", stderr)
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

private func createManifestSession(at root: URL, sessionID: String, transcriptLine: String) throws {
    let fm = FileManager.default
    try fm.createDirectory(at: root, withIntermediateDirectories: true, attributes: nil)

    let manifest: [String: Any] = [
        "session_id": sessionID,
        "generated_at_utc": "2026-03-05T00:00:00Z",
        "runtime_mode": "live",
        "session_summary": [
            "session_status": "ok",
            "duration_sec": 120
        ],
        "terminal_summary": [
            "stable_lines": [transcriptLine]
        ]
    ]
    try writeJSON(manifest, to: root.appendingPathComponent("session.manifest.json"))
    try writeData(Data([0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00]), to: root.appendingPathComponent("session.wav"))
}

private func createPendingSession(at root: URL, sessionID: String, jsonlText: String) throws {
    let fm = FileManager.default
    try fm.createDirectory(at: root, withIntermediateDirectories: true, attributes: nil)

    let pending: [String: Any] = [
        "session_id": sessionID,
        "created_at_utc": "2026-03-05T00:01:00Z",
        "mode": "record_only"
    ]
    try writeJSON(pending, to: root.appendingPathComponent("session.pending.json"))
    try writeData(Data([0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00]), to: root.appendingPathComponent("session.wav"))
    try writeData(Data(jsonlText.utf8), to: root.appendingPathComponent("session.jsonl"))
}

private func runSmoke() {
    let fm = FileManager.default
    let tempRoot = fm.temporaryDirectory
        .appendingPathComponent("recordit-search-index-smoke-\(UUID().uuidString)", isDirectory: true)
    defer { try? fm.removeItem(at: tempRoot) }

    do {
        try fm.createDirectory(at: tempRoot, withIntermediateDirectories: true, attributes: nil)
        let sessionA = tempRoot.appendingPathComponent("20260305T000000Z-live")
        let sessionB = tempRoot.appendingPathComponent("20260305T000100Z-record-only")

        try createManifestSession(
            at: sessionA,
            sessionID: "sess-a",
            transcriptLine: "[00:00.000-00:00.200] mic: alpha query line"
        )
        try createPendingSession(
            at: sessionB,
            sessionID: "sess-b",
            jsonlText: "{\"event_type\":\"final\",\"text\":\"beta fallback line\"}\n"
        )

        let service = FileSystemSessionLibraryService(
            sessionsRootProvider: { tempRoot }
        )

        let alpha = try service.listSessions(query: SessionQuery(searchText: "alpha"))
        check(alpha.count == 1, "expected one alpha search hit")
        check(alpha.first?.sessionID == "sess-a", "alpha hit should be sess-a")

        let beta = try service.listSessions(query: SessionQuery(searchText: "beta"))
        check(beta.count == 1, "expected one beta search hit")
        check(beta.first?.sessionID == "sess-b", "beta hit should be sess-b")

        let onlyOk = try service.listSessions(query: SessionQuery(status: .ok, searchText: "beta"))
        check(onlyOk.isEmpty, "status filter should exclude pending session from beta match")

        try writeData(
            Data("{\"event_type\":\"reconciled_final\",\"text\":\"delta updated line\"}\n".utf8),
            to: sessionB.appendingPathComponent("session.jsonl")
        )

        let betaAfterUpdate = try service.listSessions(query: SessionQuery(searchText: "beta"))
        check(betaAfterUpdate.isEmpty, "beta hit should disappear after jsonl update")

        let deltaAfterUpdate = try service.listSessions(query: SessionQuery(searchText: "delta"))
        check(deltaAfterUpdate.count == 1, "expected one delta search hit after update")
        check(deltaAfterUpdate.first?.sessionID == "sess-b", "delta hit should be sess-b")
    } catch {
        check(false, "smoke failed with error: \(error)")
    }
}

@main
struct SessionSearchIndexSmokeMain {
    static func main() {
        runSmoke()
        print("session_search_index_smoke: PASS")
    }
}
