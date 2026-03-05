import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("jsonl_tailer_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func append(_ text: String, to url: URL) throws {
    let handle = try FileHandle(forWritingTo: url)
    defer { try? handle.close() }
    try handle.seekToEnd()
    try handle.write(contentsOf: Data(text.utf8))
}

private func runSmoke() {
    let fm = FileManager.default
    let tempRoot = fm.temporaryDirectory
        .appendingPathComponent("recordit-jsonl-tailer-smoke-\(UUID().uuidString)", isDirectory: true)
    defer { try? fm.removeItem(at: tempRoot) }

    do {
        try fm.createDirectory(at: tempRoot, withIntermediateDirectories: true, attributes: nil)
        let jsonl = tempRoot.appendingPathComponent("session.jsonl")
        try Data(
            "{\"event_type\":\"final\",\"text\":\"first stable\"}\n{not-json}\n{\"event_type\":\"final\",\"text\":\"partial"
                .utf8
        ).write(to: jsonl, options: .atomic)

        let tailer = FileSystemJsonlTailService()

        let first = try tailer.readEvents(at: jsonl, from: .start)
        check(first.0.count == 1, "first read should return one valid event")
        check(first.0.first?.text == "first stable", "first event text mismatch")
        check(first.1.lineCount == 2, "cursor should count complete lines including malformed")
        let firstOffset = first.1.byteOffset
        check(firstOffset > 0, "cursor offset should move forward")

        try append(" trailing text\"}\n{\"event_type\":\"reconciled_final\",\"text\":\"second stable\"}\n", to: jsonl)

        let second = try tailer.readEvents(at: jsonl, from: first.1)
        check(second.0.count == 2, "second read should return completed partial + new line")
        check(second.0[0].text == "partial trailing text", "completed partial line should parse on second read")
        check(second.0[1].text == "second stable", "new stable line should parse")
        check(second.1.lineCount == 4, "cursor line count should advance by two complete lines")
        check(second.1.byteOffset > firstOffset, "cursor offset should advance on second read")

        let third = try tailer.readEvents(at: jsonl, from: second.1)
        check(third.0.isEmpty, "third read should have no duplicate rows")
        check(third.1.byteOffset == second.1.byteOffset, "cursor should stay stable with no new bytes")

        let resumedTailer = FileSystemJsonlTailService()
        let resumed = try resumedTailer.readEvents(at: jsonl, from: second.1)
        check(resumed.0.isEmpty, "persisted cursor resume should not replay stable rows")
    } catch {
        check(false, "smoke failed: \(error)")
    }
}

@main
struct JsonlTailerSmokeMain {
    static func main() {
        runSmoke()
        print("jsonl_tailer_smoke: PASS")
    }
}
