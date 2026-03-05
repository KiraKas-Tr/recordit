import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("app_environment_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private struct FailingModelResolutionService: ModelResolutionService {
    func resolveModel(_ request: ModelResolutionRequest) throws -> ResolvedModelDTO {
        throw AppServiceError(
            code: .modelUnavailable,
            userMessage: "Model is missing.",
            remediation: "Choose a valid local model path.",
            debugDetail: "backend=\(request.backend)"
        )
    }
}

private struct StubStartupMigrationRepairService: StartupMigrationRepairing {
    let report: StartupMigrationRepairReport

    func runRepair() -> StartupMigrationRepairReport {
        report
    }
}

@MainActor
private func runSmoke() async {
    let preview = AppEnvironment.preview()
    let runtimeViewModel = preview.makeRuntimeViewModel()
    let outputRoot = URL(fileURLWithPath: NSTemporaryDirectory())
        .appendingPathComponent("recordit-preview-smoke-\(UUID().uuidString)")

    await runtimeViewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
    if case let .running(processID) = runtimeViewModel.state {
        check(processID == 42, "preview runtime should use mock process id")
    } else {
        check(false, "preview runtime should transition to running using mock runtime service")
    }

    let failing = preview.replacing(modelService: FailingModelResolutionService())
    let failingRuntimeViewModel = failing.makeRuntimeViewModel()
    await failingRuntimeViewModel.startLive(outputRoot: outputRoot, explicitModelPath: nil)
    if case let .failed(error) = failingRuntimeViewModel.state {
        check(error.code == .modelUnavailable, "override model service should drive failure path")
    } else {
        check(false, "runtime should fail when overridden model service fails")
    }

    let preflightViewModel = preview.makePreflightViewModel()
    preflightViewModel.runLivePreflight()
    if case let .completed(envelope) = preflightViewModel.state {
        check(envelope.kind == "transcribe-live-preflight", "preview preflight should use fixture envelope")
    } else {
        check(false, "preview preflight should complete without spawning external binaries")
    }

    let startupReport = StartupMigrationRepairReport(
        indexPath: URL(fileURLWithPath: "/tmp/index.json"),
        startedAt: Date(timeIntervalSince1970: 1),
        completedAt: Date(timeIntervalSince1970: 2),
        timeBudgetSeconds: 1.0,
        didExceedTimeBudget: false,
        sessionCountScanned: 3,
        staleIndexEntryCount: 1,
        missingIndexEntryCount: 2,
        legacyImportCount: 1,
        truncatedSessionCount: 0,
        queryableAfterRepair: true,
        failureMessages: []
    )
    let repairedEnvironment = preview.replacing(
        startupMigrationRepairService: StubStartupMigrationRepairService(report: startupReport)
    )
    let syncReport = repairedEnvironment.runStartupMigrationRepair()
    check(syncReport?.sessionCountScanned == 3, "startup repair should run through environment")

    let asyncReport = await repairedEnvironment.scheduleStartupMigrationRepair().value
    check(asyncReport?.legacyImportCount == 1, "async startup repair should preserve report values")
}

@main
struct AppEnvironmentSmokeMain {
    static func main() async {
        await runSmoke()
        print("app_environment_smoke: PASS")
    }
}
