import Foundation

private func check(_ condition: Bool, _ message: String) {
    if !condition {
        fputs("runtime_status_mapping_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func fixtureManifest(status: String, trustNoticeCount: Int) -> SessionManifestDTO {
    SessionManifestDTO(
        sessionID: "fixture-session",
        status: status,
        runtimeMode: "live",
        trustNoticeCount: trustNoticeCount,
        artifacts: SessionArtifactsDTO(
            wavPath: URL(fileURLWithPath: "/tmp/fixture.wav"),
            jsonlPath: URL(fileURLWithPath: "/tmp/fixture.jsonl"),
            manifestPath: URL(fileURLWithPath: "/tmp/fixture.manifest.json")
        )
    )
}

private struct StaticManifestService: ManifestService {
    var manifest: SessionManifestDTO

    func loadManifest(at manifestPath: URL) throws -> SessionManifestDTO {
        manifest
    }
}

@MainActor
private func runSmoke() {
    let mapper = ManifestFinalStatusMapper()
    check(mapper.mapStatus(fixtureManifest(status: "ok", trustNoticeCount: 0)) == .ok, "ok status should map to ok")
    check(mapper.mapStatus(fixtureManifest(status: "ok", trustNoticeCount: 2)) == .degraded, "trust notices should map ok to degraded")
    check(mapper.mapStatus(fixtureManifest(status: "degraded", trustNoticeCount: 0)) == .degraded, "degraded status should remain degraded")
    check(mapper.mapStatus(fixtureManifest(status: "failed", trustNoticeCount: 0)) == .failed, "failed status should map to failed")

    let runtimeService = MockRuntimeService()
    let modelService = MockModelResolutionService(
        resolution: ResolvedModelDTO(
            resolvedPath: URL(fileURLWithPath: "/tmp/model.bin"),
            source: "smoke",
            checksumSHA256: nil,
            checksumStatus: "unknown"
        )
    )

    let okViewModel = RuntimeViewModel(
        runtimeService: runtimeService,
        manifestService: StaticManifestService(manifest: fixtureManifest(status: "ok", trustNoticeCount: 0)),
        modelService: modelService
    )
    okViewModel.loadFinalStatus(manifestPath: URL(fileURLWithPath: "/tmp/ok.manifest.json"))
    check(okViewModel.state == .completed, "ok manifest should produce completed state")

    let degradedViewModel = RuntimeViewModel(
        runtimeService: runtimeService,
        manifestService: StaticManifestService(manifest: fixtureManifest(status: "ok", trustNoticeCount: 1)),
        modelService: modelService
    )
    degradedViewModel.loadFinalStatus(manifestPath: URL(fileURLWithPath: "/tmp/degraded.manifest.json"))
    check(degradedViewModel.state == .completed, "degraded success must not map to failed state")

    let failedViewModel = RuntimeViewModel(
        runtimeService: runtimeService,
        manifestService: StaticManifestService(manifest: fixtureManifest(status: "failed", trustNoticeCount: 0)),
        modelService: modelService
    )
    failedViewModel.loadFinalStatus(manifestPath: URL(fileURLWithPath: "/tmp/failed.manifest.json"))
    guard case .failed(let error) = failedViewModel.state else {
        check(false, "failed manifest should map to failed state")
        return
    }
    check(error.code == .processExitedUnexpectedly, "failed mapping should preserve failure error code")
}

@main
struct RuntimeStatusMappingSmokeMain {
    static func main() {
        runSmoke()
        print("runtime_status_mapping_smoke: PASS")
    }
}
