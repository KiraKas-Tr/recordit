import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("model_resolution_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func makeTempDirectory() -> URL {
    let root = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
    let dir = root.appendingPathComponent("recordit-model-resolution-\(UUID().uuidString)", isDirectory: true)
    do {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    } catch {
        fputs("model_resolution_smoke failed: could not create temp dir: \(error)\n", stderr)
        exit(1)
    }
    return dir
}

@MainActor
private func runSmoke() {
    let tempDir = makeTempDirectory()
    defer { try? FileManager.default.removeItem(at: tempDir) }

    let fileModel = tempDir.appendingPathComponent("ggml-tiny.en.bin")
    let dirModel = tempDir.appendingPathComponent("whisperkit-model", isDirectory: true)

    do {
        try Data("recordit-model".utf8).write(to: fileModel)
        try FileManager.default.createDirectory(at: dirModel, withIntermediateDirectories: true)
    } catch {
        fputs("model_resolution_smoke failed: fixture setup error: \(error)\n", stderr)
        exit(1)
    }

    let resolver = FileSystemModelResolutionService(
        environment: [:],
        currentDirectoryURL: tempDir
    )

    let whisperCppResolved: ResolvedModelDTO
    do {
        whisperCppResolved = try resolver.resolveModel(
            ModelResolutionRequest(explicitModelPath: fileModel, backend: "whispercpp")
        )
    } catch {
        check(false, "whispercpp file model should resolve: \(error)")
        return
    }
    check(whisperCppResolved.source == "ui selected path", "explicit model should report ui source")
    check(whisperCppResolved.checksumStatus == "available", "file model should report available checksum")
    check((whisperCppResolved.checksumSHA256?.isEmpty == false), "file model should include checksum hash")

    do {
        _ = try resolver.resolveModel(
            ModelResolutionRequest(explicitModelPath: fileModel, backend: "whisperkit")
        )
        check(false, "whisperkit should reject file model path")
    } catch let serviceError as AppServiceError {
        check(serviceError.code == .modelUnavailable, "whisperkit wrong-kind should map to modelUnavailable")
    } catch {
        check(false, "unexpected error for whisperkit wrong-kind path")
    }

    let whisperKitResolved: ResolvedModelDTO
    do {
        whisperKitResolved = try resolver.resolveModel(
            ModelResolutionRequest(explicitModelPath: dirModel, backend: "whisperkit")
        )
    } catch {
        check(false, "whisperkit directory model should resolve: \(error)")
        return
    }
    check(whisperKitResolved.checksumStatus == "unavailable_directory", "directory model should report unavailable_directory checksum status")
    check(whisperKitResolved.checksumSHA256 == nil, "directory model should not include checksum hash")

    let missingPath = tempDir.appendingPathComponent("missing-model.bin")
    do {
        _ = try resolver.resolveModel(
            ModelResolutionRequest(explicitModelPath: missingPath, backend: "whispercpp")
        )
        check(false, "missing explicit path should fail")
    } catch let serviceError as AppServiceError {
        check(serviceError.code == .modelUnavailable, "missing explicit path should map to modelUnavailable")
    } catch {
        check(false, "unexpected error for missing explicit path")
    }

    let viewModel = ModelSetupViewModel(modelResolutionService: resolver)
    viewModel.chooseBackend("whispercpp")
    viewModel.chooseExistingModelPath(fileModel)
    check(viewModel.canStartLiveTranscribe, "valid whispercpp file path should enable live transcribe")
    check(viewModel.diagnostics?.asrModelSource == "ui selected path", "diagnostics should surface model source")
    check(viewModel.diagnostics?.asrModelChecksumStatus == "available", "diagnostics should surface checksum status")

    viewModel.chooseBackend("whisperkit")
    check(!viewModel.canStartLiveTranscribe, "invalid backend/path combination should block live transcribe")
    if case let .invalid(error) = viewModel.state {
        check(error.code == .invalidInput, "invalid backend/path should report invalidInput")
        check(
            error.userMessage.contains("model folder"),
            "invalid backend/path should include plain-language folder guidance"
        )
    } else {
        check(false, "view model should be invalid for incompatible backend/path")
    }

    let originalBackend = viewModel.selectedBackend
    viewModel.chooseBackend("moonshine")
    check(viewModel.selectedBackend == originalBackend, "unsupported backend should not replace selected backend")
    check(!viewModel.canStartLiveTranscribe, "unsupported backend should not be startable")
    if case let .invalid(error) = viewModel.state {
        check(error.code == .invalidInput, "unsupported backend should report invalidInput")
        check(error.userMessage.contains("not available"), "unsupported backend message should be plain-language")
    } else {
        check(false, "unsupported backend should produce invalid state")
    }

    let selectableBackends = Set(ModelSetupViewModel.selectableBackends)
    check(selectableBackends.contains("whispercpp"), "whispercpp should remain selectable")
    check(selectableBackends.contains("whisperkit"), "whisperkit should remain selectable")
    check(!selectableBackends.contains("moonshine"), "unsupported backends should not be selectable")
}

@main
struct ModelResolutionSmokeMain {
    static func main() async {
        await MainActor.run {
            runSmoke()
        }
        print("model_resolution_smoke: PASS")
    }
}
