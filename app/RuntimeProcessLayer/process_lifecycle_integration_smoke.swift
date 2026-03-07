import Foundation

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("process_lifecycle_integration_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private struct StaticBinaryResolver: RuntimeBinaryResolving {
    let binaries: RuntimeBinarySet

    func resolve() throws -> RuntimeBinarySet {
        binaries
    }
}

private struct StaticModelService: ModelResolutionService {
    func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        _ = request
        return ResolvedModelDTO(
            resolvedPath: URL(fileURLWithPath: "/tmp/model.bin"),
            source: "integration-smoke",
            checksumSHA256: nil,
            checksumStatus: "available"
        )
    }
}

private struct PresenceCheckedManifestService: ManifestService {
    let status: String

    func loadManifest(at manifestPath: URL) throws -> SessionManifestDTO {
        guard FileManager.default.fileExists(atPath: manifestPath.path) else {
            throw AppServiceError(
                code: .artifactMissing,
                userMessage: "Manifest missing.",
                remediation: "Retry after manifest is written."
            )
        }

        let root = manifestPath.deletingLastPathComponent()
        return SessionManifestDTO(
            sessionID: root.lastPathComponent,
            status: status,
            runtimeMode: "live",
            trustNoticeCount: 0,
            artifacts: SessionArtifactsDTO(
                wavPath: root.appendingPathComponent("session.wav"),
                jsonlPath: root.appendingPathComponent("session.jsonl"),
                manifestPath: manifestPath
            )
        )
    }
}

private func makeExecutableScript(at url: URL, body: String) throws {
    try body.write(to: url, atomically: true, encoding: .utf8)
    guard chmod(url.path, 0o755) == 0 else {
        throw AppServiceError(
            code: .ioFailure,
            userMessage: "Could not mark script as executable.",
            remediation: "Check filesystem permissions.",
            debugDetail: url.path
        )
    }
}

private func makeRuntimeService(
    recorditPath: URL,
    sequoiaPath: URL,
    stopTimeoutSeconds: TimeInterval = 1
) -> ProcessBackedRuntimeService {
    let resolver = StaticBinaryResolver(
        binaries: RuntimeBinarySet(recordit: recorditPath, sequoiaCapture: sequoiaPath)
    )
    let manager = RuntimeProcessManager(binaryResolver: resolver)
    return ProcessBackedRuntimeService(
        processManager: manager,
        pendingSidecarService: FileSystemPendingSessionSidecarService(),
        stopTimeoutSeconds: stopTimeoutSeconds,
        pendingSidecarStopTimeoutSeconds: 0.2
    )
}

private func waitForFile(at url: URL, timeoutSeconds: TimeInterval, message: String) async {
    let deadline = Date().addingTimeInterval(timeoutSeconds)
    while !FileManager.default.fileExists(atPath: url.path) && Date() < deadline {
        try? await Task.sleep(nanoseconds: 50_000_000)
    }
    check(FileManager.default.fileExists(atPath: url.path), message)
}

private func readFileData(_ url: URL, message: String) throws -> Data {
    guard let data = FileManager.default.contents(atPath: url.path) else {
        throw AppServiceError(
            code: .artifactMissing,
            userMessage: message,
            remediation: "Re-run the smoke and inspect the generated session directory.",
            debugDetail: url.path
        )
    }
    return data
}

private func readTrimmedUTF8File(_ url: URL, message: String) throws -> String {
    let data = try readFileData(url, message: message)
    return String(decoding: data, as: UTF8.self)
        .trimmingCharacters(in: .whitespacesAndNewlines)
}

private func detailValue(_ detail: String, for key: String) -> String? {
    let prefix = "\(key)="
    for token in detail.split(separator: ",") {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix(prefix) {
            return String(trimmed.dropFirst(prefix.count))
        }
    }
    return nil
}

private func requiredDetailUInt64(_ detail: String, key: String, message: String) -> UInt64 {
    guard let raw = detailValue(detail, for: key), let value = UInt64(raw) else {
        check(false, message)
        return 0
    }
    return value
}

private func requiredDetailDouble(_ detail: String, key: String, message: String) -> Double {
    guard let raw = detailValue(detail, for: key), let value = Double(raw) else {
        check(false, message)
        return 0
    }
    return value
}

@MainActor
private func runSmoke() async throws {
    let tempRoot = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
        .appendingPathComponent("recordit-process-lifecycle-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: tempRoot, withIntermediateDirectories: true)

    let binDir = tempRoot.appendingPathComponent("bin", isDirectory: true)
    try FileManager.default.createDirectory(at: binDir, withIntermediateDirectories: true)

    let liveScript = binDir.appendingPathComponent("recordit-live.sh")
    try makeExecutableScript(
        at: liveScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        trap 'exit 0' INT TERM
        while :; do :; done
        """
    )

    let crashScript = binDir.appendingPathComponent("recordit-crash.sh")
    try makeExecutableScript(
        at: crashScript,
        body: """
        #!/bin/sh
        exit 17
        """
    )

    let stubbornScript = binDir.appendingPathComponent("recordit-stubborn.sh")
    try makeExecutableScript(
        at: stubbornScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        if [ -n "$out_root" ]; then
          mkdir -p "$out_root"
        fi
        exec /usr/bin/perl -e 'my $ready = shift; if (defined $ready && length $ready) { open my $fh, ">", $ready or die $!; close $fh; } $SIG{INT}="IGNORE"; $SIG{TERM}="IGNORE"; while (1) { select undef, undef, undef, 0.1 }' "$out_root/stubborn.ready"
        """
    )

    let gracefulStopScript = binDir.appendingPathComponent("recordit-graceful-stop.sh")
    try makeExecutableScript(
        at: gracefulStopScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/graceful.ready"
        trap 'printf INT > "$out_root/stop-signal.txt"; exit 0' INT
        while :; do
          if [ -f "$out_root/session.stop.request" ]; then
            printf REQUEST > "$out_root/stop-signal.txt"
            exit 0
          fi
        done
        """
    )

    let warmupFallbackWithManifestScript = binDir.appendingPathComponent("recordit-warmup-fallback-manifest.sh")
    try makeExecutableScript(
        at: warmupFallbackWithManifestScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/warmup.ready"
        trap '' TERM
        trap 'printf INT > "$out_root/stop-signal.txt"; printf "{}" > "$out_root/session.manifest.json"; printf "partial transcript\n" > "$out_root/session.jsonl"; : > "$out_root/session.wav"; exit 0' INT
        sleep 1
        while :; do
          if [ -f "$out_root/session.stop.request" ]; then
            printf REQUEST > "$out_root/stop-signal.txt"
            printf "{}" > "$out_root/session.manifest.json"
            printf "partial transcript\n" > "$out_root/session.jsonl"
            : > "$out_root/session.wav"
            exit 0
          fi
        done
        """
    )


    let gracefulFinalizeScript = binDir.appendingPathComponent("recordit-graceful-finalize.sh")
    try makeExecutableScript(
        at: gracefulFinalizeScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/graceful-finalize.ready"
        trap 'printf INT > "$out_root/stop-signal.txt"; exit 0' INT
        while :; do
          if [ -f "$out_root/session.stop.request" ]; then
            printf REQUEST > "$out_root/stop-signal.txt"
            printf '{}' > "$out_root/session.manifest.json"
            printf 'partial transcript\n' > "$out_root/session.jsonl"
            : > "$out_root/session.wav"
            exit 0
          fi
        done
        """
    )

    let cancelPollNormalizationScript = binDir.appendingPathComponent("recordit-cancel-poll-normalization.sh")
    try makeExecutableScript(
        at: cancelPollNormalizationScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/cancel-poll.ready"
        trap 'printf INT > "$out_root/stop-signal.txt"; exit 0' INT
        while :; do :; done
        """
    )

    let interruptFallbackScript = binDir.appendingPathComponent("recordit-interrupt-fallback.sh")
    try makeExecutableScript(
        at: interruptFallbackScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/interrupt.ready"
        trap '' TERM
        trap 'printf INT > "$out_root/stop-signal.txt"; exit 0' INT
        while :; do :; done
        """
    )

    let terminateFallbackScript = binDir.appendingPathComponent("recordit-terminate-fallback.sh")
    try makeExecutableScript(
        at: terminateFallbackScript,
        body: """
        #!/bin/sh
        out_root=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --output-root) out_root="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        [ -n "$out_root" ] && mkdir -p "$out_root"
        : > "$out_root/terminate.ready"
        trap '' INT
        trap 'printf TERM > "$out_root/stop-signal.txt"; exit 0' TERM
        while :; do :; done
        """
    )

    let captureScript = binDir.appendingPathComponent("sequoia-capture.sh")
    try makeExecutableScript(
        at: captureScript,
        body: """
        #!/bin/sh
        wav_path="$2"
        mkdir -p "$(dirname "$wav_path")"
        : > "$wav_path"
        trap 'exit 0' INT TERM
        while :; do :; done
        """
    )

    let modelService = StaticModelService()

    // Live start/stop/finalize success with manifest presence.
    do {
        let processService = makeRuntimeService(recorditPath: liveScript, sequoiaPath: captureScript)
        let outputRoot = tempRoot.appendingPathComponent("live-success", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let manifestPath = outputRoot.appendingPathComponent("session.manifest.json")
        try "{}".write(to: manifestPath, atomically: true, encoding: .utf8)

        let viewModel = RuntimeViewModel(
            runtimeService: processService,
            manifestService: PresenceCheckedManifestService(status: "ok"),
            modelService: modelService,
            finalizationTimeoutSeconds: 1,
            finalizationPollIntervalNanoseconds: 10_000_000
        )

        await viewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
        guard case let .running(processID) = viewModel.state else {
            check(false, "live start should reach running state before finalization")
            return
        }
        viewModel.loadFinalStatus(manifestPath: manifestPath)
        check(
            viewModel.state == .completed,
            "live lifecycle should complete when manifest is present with ok status (state=\(String(describing: viewModel.state)))"
        )
        check(FileManager.default.fileExists(atPath: manifestPath.path), "manifest should remain present after finalization")
        _ = try? await processService.controlSession(processIdentifier: processID, action: .cancel)
    }

    // Live finalization should fail when manifest status is failed.
    do {
        let processService = makeRuntimeService(recorditPath: liveScript, sequoiaPath: captureScript)
        let outputRoot = tempRoot.appendingPathComponent("live-failed-manifest", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let manifestPath = outputRoot.appendingPathComponent("session.manifest.json")
        try "{}".write(to: manifestPath, atomically: true, encoding: .utf8)

        let viewModel = RuntimeViewModel(
            runtimeService: processService,
            manifestService: PresenceCheckedManifestService(status: "failed"),
            modelService: modelService,
            finalizationTimeoutSeconds: 1,
            finalizationPollIntervalNanoseconds: 10_000_000
        )

        await viewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
        guard case let .running(processID) = viewModel.state else {
            check(false, "live start should reach running state before failed finalization mapping")
            return
        }
        viewModel.loadFinalStatus(manifestPath: manifestPath)
        guard case let .failed(error) = viewModel.state else {
            check(false, "failed manifest status should map to failed runtime state")
            return
        }
        check(error.code == .processExitedUnexpectedly, "failed manifest status should classify as processExitedUnexpectedly")
        check(viewModel.suggestedRecoveryActions == [.openSessionArtifacts, .startNewSession], "finalized failed manifest should not advertise interruption recovery")
        check(viewModel.interruptionRecoveryContext?.outcomeClassification == .finalizedFailure, "failed manifest should classify as finalized failure")
        _ = try? await processService.controlSession(processIdentifier: processID, action: .cancel)
    }

    // Record-only lifecycle should initialize pending sidecar and allow cancel control.
    do {
        let service = makeRuntimeService(recorditPath: liveScript, sequoiaPath: captureScript)
        let outputRoot = tempRoot.appendingPathComponent("record-only", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)

        let launch = try await service.startSession(
            request: RuntimeStartRequest(
                mode: .recordOnly,
                outputRoot: outputRoot,
                inputWav: nil,
                modelPath: nil
            )
        )
        let sidecarPath = outputRoot.appendingPathComponent("session.pending.json")
        check(FileManager.default.fileExists(atPath: sidecarPath.path), "record-only launch should write pending sidecar")
        let sidecarData = try readFileData(sidecarPath, message: "record-only launch should write readable pending sidecar data")
        let sidecarJson = try JSONSerialization.jsonObject(with: sidecarData) as? [String: Any]
        check(sidecarJson?["mode"] as? String == "record_only", "pending sidecar mode should be record_only")
        check(sidecarJson?["transcription_state"] as? String == "pending_model", "pending sidecar should default to pending_model without explicit model path")

        let control = try await service.controlSession(
            processIdentifier: launch.processIdentifier,
            action: .cancel
        )
        check(control.accepted, "record-only cancel should be accepted")
    }

    // Poll path should normalize already-terminated cancel sessions the same way as direct control.
    do {
        let service = makeRuntimeService(recorditPath: cancelPollNormalizationScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("cancel-poll-normalization", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("cancel-poll.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "cancel normalization helper should signal readiness before external TERM")
        check(kill(launch.processIdentifier, SIGTERM) == 0, "external TERM should be delivered to cancel normalization helper")
        let deadline = Date().addingTimeInterval(2)
        while kill(launch.processIdentifier, 0) == 0 && Date() < deadline {
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        check(kill(launch.processIdentifier, 0) != 0, "cancel normalization helper should exit after external TERM")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .cancel)
        check(control.accepted, "cancel should accept an already-terminated SIGTERM session via the poll path")
    }

    // Launch should clear stale graceful-stop markers before runtime starts.
    do {
        let service = makeRuntimeService(recorditPath: gracefulStopScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-graceful-stop-stale-marker", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let requestPath = outputRoot.appendingPathComponent("session.stop.request")
        try Data("stale\n".utf8).write(to: requestPath, options: .atomic)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("graceful.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "launch should still reach graceful-stop readiness after clearing stale markers")
        check(!FileManager.default.fileExists(atPath: requestPath.path), "launch should clear stale graceful stop request markers")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        try? await Task.sleep(nanoseconds: 150_000_000)
        check(!FileManager.default.fileExists(atPath: signalPath.path), "stale graceful stop markers should not trigger shutdown before explicit stop")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "stop after stale-marker cleanup should be accepted")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "graceful stop helper should write stop signal after explicit stop")
        let signal = try readTrimmedUTF8File(signalPath, message: "graceful stop helper should write readable stop signal")
        check(signal == "REQUEST", "graceful stop should still honor the stop-request handshake after stale-marker cleanup")
    }

    // Unknown-process stop should fail immediately instead of burning the graceful wait budget.
    do {
        let service = makeRuntimeService(recorditPath: gracefulStopScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.6)
        let startedAt = Date()
        do {
            _ = try await service.controlSession(processIdentifier: 999_999, action: .stop)
            check(false, "unknown-process stop should fail instead of succeeding")
            return
        } catch let error as AppServiceError {
            let elapsed = Date().timeIntervalSince(startedAt)
            check(error.code == .runtimeUnavailable, "unknown-process stop should surface runtimeUnavailable")
            check(elapsed < 0.2, "unknown-process stop should not wait for graceful timeout budget before failing")
        }
    }

    // Immediate stop after launch should settle via graceful-timeout -> interrupt fallback with deterministic telemetry.
    do {
        let service = makeRuntimeService(recorditPath: liveScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-immediate-stop", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "immediate stop should be accepted")
        check(control.detail.contains("stop_strategy=interrupt_fallback"), "immediate stop should escalate to interrupt fallback when graceful handshake cannot complete")
        check(control.detail.contains("escalation_reason=graceful_handshake_timeout"), "immediate stop should report graceful-timeout escalation reason")
        let gracefulWaitMs = requiredDetailUInt64(control.detail, key: "graceful_wait_ms", message: "immediate stop should report graceful_wait_ms telemetry")
        let interruptWaitMs = requiredDetailUInt64(control.detail, key: "interrupt_wait_ms", message: "immediate stop should report interrupt_wait_ms telemetry")
        let terminateWaitMs = requiredDetailUInt64(control.detail, key: "terminate_wait_ms", message: "immediate stop should report terminate_wait_ms telemetry")
        check(gracefulWaitMs > 0, "immediate stop should consume some graceful wait budget before fallback")
        check(interruptWaitMs > 0, "immediate stop should consume interrupt wait budget once fallback starts")
        check(terminateWaitMs == 0, "immediate stop should not consume terminate budget when interrupt fallback succeeds")
    }

    // Warmup stop should also follow deterministic fallback timing and signal INT while runtime is not yet active.
    do {
        let service = makeRuntimeService(recorditPath: warmupFallbackWithManifestScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-warmup-stop", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("warmup.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "warmup helper should report readiness marker before stop")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "warmup stop should be accepted")
        check(control.detail.contains("stop_strategy=interrupt_fallback"), "warmup stop should escalate to interrupt fallback when handshake cannot complete during warmup")
        check(control.detail.contains("escalation_reason=graceful_handshake_timeout"), "warmup stop should report graceful-timeout escalation reason")
        let gracefulWaitMs = requiredDetailUInt64(control.detail, key: "graceful_wait_ms", message: "warmup stop should report graceful_wait_ms telemetry")
        let interruptWaitMs = requiredDetailUInt64(control.detail, key: "interrupt_wait_ms", message: "warmup stop should report interrupt_wait_ms telemetry")
        check(gracefulWaitMs > 0, "warmup stop should burn graceful wait budget before fallback")
        check(interruptWaitMs > 0, "warmup stop should burn interrupt wait budget on fallback")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "warmup fallback helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "warmup fallback helper should write readable stop signal")
        check(signal == "INT", "warmup fallback should deliver INT while runtime remains in warmup")
    }

    // Graceful stop should prefer the drain/finalization handshake before interrupt fallback.
    do {
        let service = makeRuntimeService(recorditPath: gracefulStopScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-graceful-stop", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("graceful.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "graceful stop helper should signal readiness before stop")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "graceful stop should be accepted")
        check(control.detail.contains("stop_strategy=graceful_handshake"), "graceful stop should report graceful handshake strategy metadata")
        check(control.detail.contains("graceful_request_written=true"), "graceful stop diagnostics should report graceful request marker write")
        check(control.detail.contains("escalation_reason=none"), "graceful stop should report no escalation reason")
        let gracefulWaitMs = requiredDetailUInt64(control.detail, key: "graceful_wait_ms", message: "graceful stop should report graceful_wait_ms telemetry")
        let interruptWaitMs = requiredDetailUInt64(control.detail, key: "interrupt_wait_ms", message: "graceful stop should report interrupt_wait_ms telemetry")
        let terminateWaitMs = requiredDetailUInt64(control.detail, key: "terminate_wait_ms", message: "graceful stop should report terminate_wait_ms telemetry")
        let gracefulTimeoutSeconds = requiredDetailDouble(control.detail, key: "graceful_timeout_seconds", message: "graceful stop should report graceful timeout budget")
        check(gracefulWaitMs > 0, "graceful stop should spend some graceful wait budget before completion")
        check(interruptWaitMs == 0, "graceful stop should not spend interrupt budget")
        check(terminateWaitMs == 0, "graceful stop should not spend terminate budget")
        check(gracefulTimeoutSeconds > 0, "graceful stop should expose a positive graceful timeout budget")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "graceful stop helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "graceful stop helper should write readable stop signal")
        check(signal == "REQUEST", "stop should honor the graceful stop-request handshake before forced fallback")
        let requestPath = outputRoot.appendingPathComponent("session.stop.request")
        check(!FileManager.default.fileExists(atPath: requestPath.path), "stop handling should clean up the graceful stop request marker once control settles")
    }

    // Marker-driven graceful stop should also drive RuntimeViewModel finalization to completion.
    do {
        let processService = makeRuntimeService(recorditPath: gracefulFinalizeScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-graceful-stop-finalize", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)

        let viewModel = RuntimeViewModel(
            runtimeService: processService,
            manifestService: PresenceCheckedManifestService(status: "ok"),
            modelService: modelService,
            finalizationTimeoutSeconds: 1,
            finalizationPollIntervalNanoseconds: 10_000_000
        )

        await viewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
        guard case .running = viewModel.state else {
            check(false, "marker-driven graceful finalize smoke should reach running state before stop")
            return
        }
        let readyPath = outputRoot.appendingPathComponent("graceful-finalize.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "graceful finalize helper should signal readiness before stop")
        await viewModel.stopCurrentRun()
        check(viewModel.state == .completed, "marker-driven graceful stop should finalize to completed state")
        check(viewModel.suggestedRecoveryActions.isEmpty, "successful graceful stop finalization should not advertise recovery actions")
        check(viewModel.interruptionRecoveryContext == nil, "successful graceful stop finalization should not retain interruption recovery context")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "graceful finalize helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "graceful finalize helper should write readable stop signal")
        check(signal == "REQUEST", "marker-driven stop should still prefer the graceful request handshake on the finalization path")
        let manifestPath = outputRoot.appendingPathComponent("session.manifest.json")
        check(FileManager.default.fileExists(atPath: manifestPath.path), "marker-driven graceful stop should leave a final manifest for bounded finalization")
        let transcriptPath = outputRoot.appendingPathComponent("session.jsonl")
        check(FileManager.default.fileExists(atPath: transcriptPath.path), "marker-driven graceful stop should retain transcript artifacts for the finalized session")
        let requestPath = outputRoot.appendingPathComponent("session.stop.request")
        check(!FileManager.default.fileExists(atPath: requestPath.path), "marker-driven graceful stop should clean up the stop-request marker after finalization")
    }

    // Stop should fall back to interrupt when the graceful handshake does not complete.
    do {
        let service = makeRuntimeService(recorditPath: interruptFallbackScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-interrupt-fallback", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("interrupt.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "interrupt fallback helper should signal readiness before stop")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "interrupt fallback stop should be accepted")
        check(control.detail.contains("stop_strategy=interrupt_fallback"), "interrupt fallback should emit explicit strategy diagnostics")
        check(control.detail.contains("escalation_reason=graceful_handshake_timeout"), "interrupt fallback should record graceful-timeout escalation reason")
        let gracefulWaitMs = requiredDetailUInt64(control.detail, key: "graceful_wait_ms", message: "interrupt fallback should report graceful_wait_ms telemetry")
        let interruptWaitMs = requiredDetailUInt64(control.detail, key: "interrupt_wait_ms", message: "interrupt fallback should report interrupt_wait_ms telemetry")
        let terminateWaitMs = requiredDetailUInt64(control.detail, key: "terminate_wait_ms", message: "interrupt fallback should report terminate_wait_ms telemetry")
        check(gracefulWaitMs > 0, "interrupt fallback should spend graceful wait budget before escalation")
        check(interruptWaitMs > 0, "interrupt fallback should spend interrupt wait budget")
        check(terminateWaitMs == 0, "interrupt fallback should not spend terminate wait budget when INT succeeds")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "interrupt fallback helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "interrupt fallback helper should write readable stop signal")
        check(signal == "INT", "stop should fall back to INT when graceful TERM handshake stalls")
    }

    // Stop should escalate to terminate when interrupt fallback does not complete.
    do {
        let service = makeRuntimeService(recorditPath: terminateFallbackScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-terminate-fallback", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let readyPath = outputRoot.appendingPathComponent("terminate.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "terminate fallback helper should signal readiness before stop")
        let control = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
        check(control.accepted, "terminate fallback stop should be accepted")
        check(control.detail.contains("stop_strategy=terminate_fallback"), "terminate fallback should emit explicit strategy diagnostics")
        check(control.detail.contains("escalation_reason=interrupt_timeout"), "terminate fallback should report interrupt-timeout escalation reason")
        let gracefulWaitMs = requiredDetailUInt64(control.detail, key: "graceful_wait_ms", message: "terminate fallback should report graceful_wait_ms telemetry")
        let interruptWaitMs = requiredDetailUInt64(control.detail, key: "interrupt_wait_ms", message: "terminate fallback should report interrupt_wait_ms telemetry")
        let terminateWaitMs = requiredDetailUInt64(control.detail, key: "terminate_wait_ms", message: "terminate fallback should report terminate_wait_ms telemetry")
        check(gracefulWaitMs > 0, "terminate fallback should spend graceful wait budget before escalation")
        check(interruptWaitMs > 0, "terminate fallback should spend interrupt wait budget before TERM escalation")
        check(terminateWaitMs > 0, "terminate fallback should spend terminate wait budget before process exits")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "terminate fallback helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "terminate fallback helper should write readable stop signal")
        check(signal == "TERM", "terminate fallback should deliver TERM after interrupt fallback stalls")
    }

    // Post-fallback finalization should still classify manifest-driven failures correctly.
    do {
        let processService = makeRuntimeService(recorditPath: warmupFallbackWithManifestScript, sequoiaPath: captureScript, stopTimeoutSeconds: 0.4)
        let outputRoot = tempRoot.appendingPathComponent("live-warmup-fallback-failed-manifest", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)

        let viewModel = RuntimeViewModel(
            runtimeService: processService,
            manifestService: PresenceCheckedManifestService(status: "failed"),
            modelService: modelService,
            finalizationTimeoutSeconds: 1,
            finalizationPollIntervalNanoseconds: 10_000_000
        )

        await viewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
        let readyPath = outputRoot.appendingPathComponent("warmup.ready")
        await waitForFile(at: readyPath, timeoutSeconds: 2, message: "warmup fallback manifest helper should report readiness marker before stop")
        await viewModel.stopCurrentRun()
        guard case let .failed(error) = viewModel.state else {
            check(false, "post-fallback failed manifest should map to failed runtime state")
            return
        }
        check(error.code == .processExitedUnexpectedly, "post-fallback failed manifest should classify as processExitedUnexpectedly")
        check(viewModel.interruptionRecoveryContext?.outcomeClassification == .finalizedFailure, "post-fallback failed manifest should classify as finalized failure")
        check(viewModel.suggestedRecoveryActions == [.openSessionArtifacts, .startNewSession], "post-fallback failed manifest should keep finalized-failure recovery actions")
        let signalPath = outputRoot.appendingPathComponent("stop-signal.txt")
        await waitForFile(at: signalPath, timeoutSeconds: 1, message: "post-fallback helper should write stop signal")
        let signal = try readTrimmedUTF8File(signalPath, message: "post-fallback helper should write readable stop signal")
        check(signal == "INT", "post-fallback manifest classification scenario should still stop via INT fallback during warmup")
    }

    // Crash branch should map to processExitedUnexpectedly.
    do {
        let service = makeRuntimeService(recorditPath: crashScript, sequoiaPath: captureScript)
        let outputRoot = tempRoot.appendingPathComponent("live-crash", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        try? await Task.sleep(nanoseconds: 100_000_000)

        do {
            _ = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
            check(false, "crash branch should throw processExitedUnexpectedly")
        } catch let serviceError as AppServiceError {
            check(serviceError.code == .processExitedUnexpectedly, "crash branch should classify as processExitedUnexpectedly")
        }
    }

    // Timeout branch should map to timeout.
    do {
        let service = makeRuntimeService(
            recorditPath: stubbornScript,
            sequoiaPath: captureScript,
            stopTimeoutSeconds: 0.2
        )
        let outputRoot = tempRoot.appendingPathComponent("live-timeout", isDirectory: true)
        try FileManager.default.createDirectory(at: outputRoot, withIntermediateDirectories: true)
        let launch = try await service.startSession(
            request: RuntimeStartRequest(mode: .live, outputRoot: outputRoot)
        )
        let stubbornReady = outputRoot.appendingPathComponent("stubborn.ready")
        let readyDeadline = Date().addingTimeInterval(2)
        while !FileManager.default.fileExists(atPath: stubbornReady.path) && Date() < readyDeadline {
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        check(FileManager.default.fileExists(atPath: stubbornReady.path), "timeout helper should signal readiness before stop")

        do {
            _ = try await service.controlSession(processIdentifier: launch.processIdentifier, action: .stop)
            check(false, "timeout branch should throw timeout")
        } catch let serviceError as AppServiceError {
            check(serviceError.code == .timeout, "timeout branch should classify as timeout")
            let debugDetail = serviceError.debugDetail ?? ""
            check(debugDetail.contains("stop_strategy=terminate_timeout"), "timeout diagnostics should report terminate-timeout stop strategy")
            check(debugDetail.contains("escalation_reason=interrupt_timeout"), "timeout diagnostics should report interrupt-timeout escalation reason")
        }
    }
}

@main
struct ProcessLifecycleIntegrationSmokeMain {
    static func main() async {
        do {
            try await runSmoke()
            print("process_lifecycle_integration_smoke: PASS")
        } catch {
            fputs("process_lifecycle_integration_smoke failed: \(error)\n", stderr)
            exit(1)
        }
    }
}
