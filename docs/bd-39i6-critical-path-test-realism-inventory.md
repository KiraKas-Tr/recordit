# bd-39i6 — Critical-path test realism inventory

This artifact inventories the **critical product/runtime surfaces** that matter for truthful coverage claims, rather than every single test file in the repository. The purpose is to answer a narrower but more operational question:

> For each high-value user/runtime journey, what current lane covers it, how realistic is that lane, and what still remains unproven?

Structured support data lives in `artifacts/test-inventory/bd-39i6-critical-surface-inventory.csv`.

## Realism vocabulary used here

- `mock` — in-process doubles such as `MockRuntimeService`, `MockSessionLibraryService`, or preview-only environment replacements.
- `fixture` — frozen JSONL / WAV / model inputs, or deterministic prerecorded payloads.
- `temp-filesystem` — test creates temporary directories/files or shell-script executables to model runtime/process behavior.
- `scripted` — orchestrated subprocess/package/UI harness that still bypasses production reality with scripted services, `RECORDIT_UI_TEST_MODE`, `/usr/bin/true`, or fake capture.
- `uncovered` — no canonical current lane found.

## Layer vocabulary used in the CSV

- `integration` — Rust integration tests and standalone Swift smoke binaries that exercise multi-component behavior without going through XCTest.
- `xctest` — unit-test target coverage running inside the XCTest target.
- `xcuitest` — UI test target coverage running through XCUITest.
- `scripted-e2e` — shell-driven or package-driven orchestration that retains logs/artifacts and may exercise built products.
- `gap` — intentionally used for surfaces where no canonical current lane was found.
- Gap rows still use stable unique `lane_name` values in the CSV so downstream tooling can key them without collisions.

## Executive read

1. **The repo has meaningful coverage, but most critical surfaces are not proven under fully production-real conditions.**
2. **The biggest realism seam in the app layer is `RECORDIT_UI_TEST_MODE` + `AppEnvironment.preview()` + scripted runtime/preflight replacements.**
3. **The biggest realism seam in the runtime layer is deterministic fake capture (`RECORDIT_FAKE_CAPTURE_FIXTURE`) instead of true device capture.**
4. **The biggest outright gaps are DMG install/open, production-environment app journey coverage, and functional playback verification.**

## Critical-surface matrix

| Surface | Current lane(s) | Current realism | What is actually proven | What is still unproven / misleading |
|---|---|---|---|---|
| First-run onboarding progression | `app/RecorditAppUITests/RecorditAppUITests.swift` (`testFirstRunOnboardingHappyPathTransitionsToMainRuntime`), `app/AppShell/onboarding_completion_smoke.swift` | `scripted` | Onboarding step flow, completion gating, transition into main runtime shell | UI test runs under `RECORDIT_UI_TEST_MODE` and preview-style environment; it does **not** prove production runtime startup |
| Permission remediation journey | `app/RecorditAppUITests/RecorditAppUITests.swift` (`testPermissionDenialRemediationRecoversToOnboardingProgression`, `testPermissionCheckFailureStillShowsDeepLinksAndMissingRows`), `app/AppShell/permission_remediation_smoke.swift` | `scripted` / `fixture` | Permission rows, deep links, restart advisory, and re-check progression | No real TCC prompt/system-settings round trip is exercised |
| Startup runtime readiness for returning users | `app/RecorditAppTests/RecorditAppTests.swift`, `app/RuntimeProcessLayer/runtime_binary_readiness_smoke.swift` | `mock` / `temp-filesystem` | Invalid override rejection, bundled binary lookup semantics, recovery routing | No packaged-app startup lane proves the signed embedded payloads are what the app actually resolves at runtime |
| Preflight contract and gating policy | `app/Preflight/preflight_gating_smoke.swift`, `app/RecorditAppTests/RecorditAppTests.swift` | `fixture` / `mock` | Readiness ID vocabulary, blocking/warn/fallback policy, preview preflight envelope shape | No full app lane proves production preflight in the shipped app context end-to-end |
| Live run start/stop and summary UI | `app/RecorditAppUITests/RecorditAppUITests.swift` (`testLiveRunStartStopShowsRuntimeStatusTranscriptAndSummary`), `app/RecorditAppTests/RecorditAppTests.swift` (`testRuntimeViewModelStartStopFinalizationCompletes`) | `scripted` / `mock` | UI state transitions and view-model finalization mapping | UI lane uses scripted runtime + `/usr/bin/true`; no production app lane proves real runtime launch, transcript generation, and final summary end-to-end |
| Stop failure recovery affordances | `app/RecorditAppUITests/RecorditAppUITests.swift` (`testRuntimeStopFailureShowsRecoveryAffordances`), `app/ViewModels/runtime_stop_finalization_smoke.swift` | `scripted` / `mock` | Recovery copy/actions for stop failure and bounded finalization failure | No packaged-app failure path proves real process hang/crash + retained evidence through full UI |
| Runtime process lifecycle | `app/RuntimeProcessLayer/process_lifecycle_integration_smoke.swift`, `app/Integration/process_lifecycle_soak_smoke.swift` | `temp-filesystem` | Start/stop/cancel behavior and lifecycle stability against shell-script stand-ins | These are not the shipped `recordit` / `sequoia_capture` binaries |
| Record-only pending queue promotion | `app/Services/pending_queue_integration_smoke.swift` | `temp-filesystem` | Sidecar promotion, failed retry context, pending→transcribing→completed/failed transitions | Not app-driven and not backed by production runtime binaries |
| Session export | `app/Exports/export_smoke.swift`, `app/Exports/session_export_view_model_smoke.swift` | `fixture` / `mock` | Export file naming, request shaping, diagnostics options, and redaction/defaults semantics | No end-to-end UI export lane from real recorded session through app shell |
| Search/list/navigation | `app/Services/session_search_index_smoke.swift`, `app/Navigation/navigation_smoke.swift`, `app/ViewModels/session_list_smoke.swift` | `temp-filesystem` / `mock` | Search indexing/query semantics, list filtering/action gating, and navigation coordination | No production app journey validates search/list/detail wiring against a realistic session corpus |
| Playback | `app/Accessibility/accessibility_smoke.swift` | `mock` | Accessibility metadata for playback controls exists | `SessionPlaybackViewModel` and `SessionAudioPlaybackService` are implemented, but no functional playback lane was found for load/play/pause/seek/audio-device behavior |
| Rust live runtime behavior | `tests/live_stream_true_live_integration.rs`, `tests/bd_1n5v_contract_regression.rs`, `scripts/gate_v1_acceptance.sh` | `fixture` / `scripted` | `transcribe-live` emits stable events, grows artifacts, and preserves declared semantics under deterministic runs | All current “live” lanes still rely on `RECORDIT_FAKE_CAPTURE_FIXTURE`; they do not prove true device capture |
| Rust offline/replay contract | `tests/representative_offline_request_regression.rs`, `tests/historical_replay_compat_regression.rs`, `tests/transcribe_live_legacy_entrypoints_compat.rs` | `fixture` | Offline request path, replay compatibility, and legacy entrypoint contracts | Valuable, but these lanes should not be mistaken for app-level E2E coverage |
| Packaged release-context integrity | `scripts/verify_recordit_release_context.sh` | `scripted` | Codesign/entitlements/bundle inventory/payload presence and packaged preflight evidence | Does not open the app UI or complete a user journey |
| Packaged live smoke | `scripts/gate_packaged_live_smoke.sh` | `scripted` | Embedded runtime in packaged `Recordit.app` can run a deterministic live-smoke flow and emit artifacts | Still fake-capture-backed; not true device capture or UI-driven launch |
| DMG creation | `scripts/create_recordit_dmg.sh` | `scripted` | DMG is composed with `Recordit.app` plus `Applications` symlink | No mount/open/install/launch verification is included |
| DMG install/open journey | No canonical current lane found | `uncovered` | Nothing canonical today | Mount, inspect, install, first launch, and retained logs remain unproven |
| Production-environment app journey | No canonical current lane found | `uncovered` | Nothing canonical today | No lane launches the production app environment without `RECORDIT_UI_TEST_MODE` / preview seams and completes onboarding → live run → session review |

## Biggest overclaim risks today

### 1. Treating XCUITest as production-real app coverage

Current XCUITest coverage is useful, but it is **not** production-real because `app/RecorditApp/RecorditApp.swift` switches to `AppEnvironment.preview()` when `RECORDIT_UI_TEST_MODE=1`, then optionally swaps in `ScriptedPreflightCommandRunner` and `ScriptedUITestRuntimeService`.

That means these lanes validate UI flow and copy, not the production runtime/process boundary.

### 2. Treating “live” runtime tests as true-capture proof

Several Rust and shell lanes execute the real `transcribe-live` binary, which is important, but they still inject capture via `RECORDIT_FAKE_CAPTURE_FIXTURE`. That is stronger than pure mocks, but weaker than device-real capture.

### 3. Treating packaging audits as app-journey proof

`scripts/verify_recordit_release_context.sh` is strong evidence for signing/payload parity, but it is not equivalent to “installed Recordit.app works for a user.”

## Priority gaps this inventory exposes

1. **Production-backed app-shell/runtime lane** without `MockServices`, preview DI, or `RECORDIT_UI_TEST_MODE` shortcuts.
2. **Real-filesystem session/artifact lane** covering creation, persistence, finalization, and later app consumption.
3. **Standardized e2e evidence contract** so all lanes retain comparable logs/artifacts.
4. **DMG install/mount/open verification** with detailed retained evidence.
5. **Functional playback verification** against real `session.wav` artifacts in app context.

## Recommended interpretation rule

Until the uncovered rows above gain canonical lanes, the most accurate project-level statement is:

> Recordit has substantial contract, smoke, and scripted verification coverage, but it does **not yet** have complete mock-free critical-path coverage or one comprehensive production-real end-to-end app journey suite.
