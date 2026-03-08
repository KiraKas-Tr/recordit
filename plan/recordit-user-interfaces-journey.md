# Recordit Desktop UX/UI Journey (v1 Spec)

Last updated: 2026-03-04

## Product Goal
Ship a DMG-first macOS desktop experience where users can install, grant permissions, record, and transcribe without touching Terminal.

## UX Principles
1. No-terminal workflow by default.
2. One obvious happy path.
3. Clear status during capture/transcription.
4. Degradation is visible but non-technical.
5. Fast recovery when permission/model/network issues happen.

## Primary User Stories
1. As a user, I can download and install a DMG app without cloning the repo or running build commands.
2. As a user, first launch guides me through required setup (permissions + model assets).
3. As a user, I can do everything from the app window: start, stop, and review output.
4. As a user, I can choose `Live Transcribe` or `Record Only`.
5. As a user, I can still record if I am offline, then transcribe later when models are available.

## Out of Scope for v1
- Full session library/search/replay manager.
- Full advanced diagnostics UI for all runtime flags.
- Auto-update system.

## Technical Architecture (v1)

### Runtime Topology
1. `Recordit.app` (SwiftUI shell) is the only user-facing process.
2. Rust runtime binaries remain execution engines:
   - `recordit` for live/offline transcription orchestration.
   - `sequoia_capture` for record-only sessions.
3. UI does not parse terminal text; UI consumes structured artifacts:
   - `session.jsonl` (streaming + append-only runtime events)
   - `session.manifest.json` (canonical end-of-run summary)
   - `session.wav` (canonical audio output)

### Component Boundaries
1. `AppShell` (SwiftUI App)
   - window lifecycle
   - route first run vs normal run
2. `OnboardingCoordinator`
   - permission checks/remediation
   - model readiness
3. `SessionCoordinator`
   - starts/stops child processes
   - owns session state machine
4. `RuntimeProcessManager`
   - command construction
   - subprocess supervision
   - exit code + crash handling
5. `JsonlTailService`
   - incremental read of appended rows
   - event decoding and UI event fanout
6. `ManifestService`
   - parse and validate final manifest fields
7. `ModelResolutionManager`
   - resolve model path using runtime precedence (`--model` > `RECORDIT_ASR_MODEL` > backend defaults)
   - surface resolved-path source + checksum status diagnostics from preflight/runtime manifests
   - optional app-layer bootstrap hook (outside current runtime contract)

### Module Boundary Governance (bd-2zxu)
Canonical ownership and dependency-direction rules are defined in:

- `docs/architecture-swiftui-module-map.md`

Required guardrails for implementation/review:

1. Views and view-models must not spawn runtime processes directly.
2. Views and view-models must not parse JSONL/manifest artifacts directly.
3. Process launch/supervision remains isolated to service/process-layer modules.
4. PRs that touch architecture boundaries must pass the boundary checklist in `docs/architecture-swiftui-module-map.md`.

### Process Contracts
All commands are spawned by the app with absolute paths.

1. Live mode command
```bash
recordit run \
  --mode live \
  --output-root <session_root_abs> \
  --language <language_tag> \
  --profile <quality_profile> \
  --model <model_abs_path> \
  --json
```
Note: unbounded live sessions must omit `--duration-sec` in `recordit`; `0` is rejected at the operator CLI layer.

2. Record-only command
```bash
sequoia_capture \
  0 \
  <session_root_abs>/session.wav \
  48000 \
  adapt-stream-rate \
  warn
```

3. Deferred offline transcription command
```bash
recordit run \
  --mode offline \
  --input-wav <session_root_abs>/session.wav \
  --output-root <session_root_abs> \
  --language <language_tag> \
  --profile <quality_profile> \
  --model <model_abs_path> \
  --json
```

### Session Artifact Contract
Per session root:
1. `session.input.wav` (live progressive capture input, when applicable)
2. `session.wav` (canonical final audio artifact)
3. `session.jsonl` (runtime events; append-only)
4. `session.manifest.json` (canonical summary + trust/degradation)

UI must treat manifest as source of truth for final status.

### JSONL Event Mapping
UI behavior by `event_type`:
1. `partial` -> inline transient transcript row/update.
2. `final` -> stable transcript line.
3. `llm_final` -> stable transcript line with cleanup badge.
4. `reconciled_final` -> stable transcript line with reconciliation badge.
5. `mode_degradation` -> non-blocking warning card.
6. `trust_notice` -> trust banner/detail item.
7. `lifecycle_phase` -> state transition indicator.
8. `chunk_queue`, `asr_worker_pool`, `cleanup_queue` -> hidden by default, visible in details drawer.
9. `vad_boundary`, `reconciliation_matrix` -> hidden control events used for ordering/reconciliation diagnostics, not primary transcript rows.
10. Transcript-event decoding contract:
    - required fields for transcript rows: `event_type`, `channel`, `segment_id`, `start_ms`, `end_ms`, `text`, `asr_backend`, `vad_boundary_count`
    - unknown extra fields must be ignored (forward-compatible)
    - unknown future `event_type` values must be ignored without terminating UI streaming

### Exit and Status Semantics
1. Exit `0` can be:
   - nominal success
   - degraded success
2. Exit `2` means failure path.
3. UI final badge selection MUST be manifest-driven:
   - `Failed`: process exit non-zero OR missing/invalid runtime manifest
   - `Degraded`: runtime manifest valid AND (`session_summary.session_status == "degraded"` OR `trust.degraded_mode_active == true` OR `trust.notice_count > 0`)
   - `OK`: runtime manifest valid AND `session_summary.session_status == "ok"` AND `trust.notice_count == 0`

### Permission Integration (macOS)
Required permissions:
1. Screen Recording (ScreenCaptureKit source availability)
2. Microphone

Technical checks:
1. Onboarding preflight command:
   - `recordit preflight --mode live --output-root <preflight_root_abs> --json`
2. UI preflight parser must validate manifest envelope before consuming payload:
   - `kind == "transcribe-live-preflight"`
   - `schema_version == "1"`
3. UI preflight mapping must consume manifest/check fields:
   - `overall_status` (`PASS`, `WARN`, `FAIL`)
   - `config.runtime_mode`
   - `config.runtime_mode_taxonomy`
   - `config.runtime_mode_selector`
   - `config.runtime_mode_status`
   - `checks[]` (`id`, `status`, `detail`, `remediation`)
4. Minimum check IDs to map in UI:
   - `screen_capture_access` or `display_availability`
   - `microphone_access`
   - `model_path`
   - `out_wav`
   - `out_jsonl`
   - `out_manifest`
   - `sample_rate`
   - `backend_runtime`
5. Blocking semantics for `Live Transcribe`:
   - block if any of these checks is `FAIL`: `model_path`, `out_wav`, `out_jsonl`, `out_manifest`, `screen_capture_access`/`display_availability`, `microphone_access`
   - `backend_runtime` and `sample_rate` are `WARN`-eligible and may proceed only with explicit user acknowledgment

### Model Asset Technical Spec
1. Default model profile is pre-selected (`tiny.en` equivalent).
2. v1 runtime contract consumes local model paths; UI must resolve path selection to runtime-compatible absolute paths.
3. Model bootstrap/source acquisition is currently external to runtime contract (for example setup workflow); UI downloader is optional app-layer feature and must not change CLI contract assumptions.
4. Model path resolution precedence must match runtime:
   - `--model` (highest)
   - `RECORDIT_ASR_MODEL`
   - backend defaults
5. Backend support matrix in v1 UI:
   - `whispercpp` supported
   - `whisperkit` supported
   - `moonshine` not wired; do not expose as ready/selectable backend
6. Backend model kind constraints:
   - `whispercpp` -> file path
   - `whisperkit` -> directory path
7. Model diagnostics surfaced in UI should include:
   - `asr_model`
   - `asr_model_source`
   - `asr_model_checksum_sha256` (when available)
   - `asr_model_checksum_status` (`available`, `unavailable_directory`, `unavailable_not_file`, `unavailable_unresolved`, `unavailable_checksum_error`)
8. If offline/no model:
   - enable `Record Only`
   - disable `Live Transcribe` with explicit reason

### Session State Machine (Technical)
1. `Idle`
2. `PreparingSession`
3. `RunningLive` or `RunningRecordOnly`
4. `Stopping`
5. `FinalizingArtifacts`
6. `CompletedOk` / `CompletedDegraded` / `CompletedFailed`

Transition constraints:
1. `Start` allowed only from `Idle`.
2. `Stop` allowed only from active running states.
3. No second `Start` until finalization finishes.
4. On process crash, transition to `CompletedFailed` with recovery actions.
5. Runtime lifecycle mapping from JSONL (`lifecycle_phase`) must be surfaced:
   - `warmup`, `active`, `draining`, `shutdown`
6. Transcript readiness during run must use `ready_for_transcripts`.

### UI Data Model (Implementation Types)
1. `UiSessionConfig`
   - mode, language, quality profile, model id/path
2. `UiRuntimeState`
   - phase, timer, transcript buffer, health/trust summary
3. `UiOnboardingState`
   - permission states + model setup status
4. `UiSessionArtifacts`
   - absolute paths + existence flags + timestamps
5. `UiFailureContext`
   - failure class, remediation actions, diagnostics token

### Reliability Requirements
1. Live transcript rows should appear during active runtime, not only post-run.
2. `Stop` must produce finalized manifest/jsonl within bounded timeout.
3. App must never depend on parsing colored terminal output.
4. JSONL tailer must parse only newline-terminated rows; trailing partial row bytes are buffered and retried.
5. UI should survive malformed JSONL rows by skipping bad lines, incrementing diagnostics counters, and continuing stream.
6. Tailer must persist cursor state per active session (`byte_offset`, `line_count`, `last_mtime`) for resumable reads.
7. Decoder must preserve append-only ordering and never rewrite previously emitted stable rows.

### Security and Privacy Requirements
1. Audio/transcripts remain local by default.
2. No automatic cloud upload in v1.
3. Paths shown in UI must be user-safe and absolute.
4. Logs/diagnostics must not include full transcript text unless user explicitly exports diagnostics.

## End-to-End Journey

### 1) Install Journey
1. User downloads `Recordit.dmg`.
2. User drags app to `Applications`.
3. User launches app from Finder/Launchpad.
4. App opens to onboarding (if first run).

### 2) First-Run Onboarding Journey
1. Welcome screen explains "Record + transcribe without Terminal."
2. Permissions screen requests:
   - Screen Recording
   - Microphone
3. Model setup screen:
   - detect and validate a runtime-compatible local model path
   - allow `Choose Existing Model` and optional guided download/bootstrap path
   - show progress + retry for any app-layer bootstrap operation
4. Ready screen confirms setup complete and moves user to main recording UI.

### 3) Main Runtime Journey
1. User chooses mode:
   - `Live Transcribe`
   - `Record Only`
2. User taps `Start`.
3. During session:
   - timer + state badge visible
   - live mode shows streaming transcript
   - record-only mode shows "transcript pending"
4. User taps `Stop`.
5. App performs graceful finalize and shows session summary:
   - status: `OK` / `Degraded` / `Failed`
   - transcript availability
   - open artifacts action

### 4) Offline Journey
1. If model not available and internet is down:
   - app allows `Record Only` mode
   - app explains transcription will run later
2. When model becomes available:
   - app shows "Transcribe pending session" action.

## Screen Inventory
1. `OnboardingWelcomeView`
2. `OnboardingPermissionsView`
3. `OnboardingModelSetupView`
4. `OnboardingReadyView`
5. `MainSessionView`
6. `SessionSummarySheet`
7. `ErrorRecoverySheet`

## Desktop UI ASCII Art

### A) Onboarding Wizard (Permissions + Model Setup)
```text
+----------------------------------------------------------------------------------+
| Recordit Setup                                                              (1/4) |
+----------------------------------------------------------------------------------+
| Welcome                                                                          |
| Record and transcribe system + mic audio without using Terminal.                |
|                                                                                  |
| Requirements                                                                     |
| [ ] Screen Recording Permission        [Open Settings] [Re-check]                |
| [ ] Microphone Permission              [Open Settings] [Re-check]                |
|                                                                                  |
| Model Setup                                                                       |
| Resolved model path: /Users/.../models/whispercpp/ggml-tiny.en.bin              |
| Source: env RECORDIT_ASR_MODEL     Checksum: available                            |
| [Choose Existing Model]   [Guided Setup]   [Re-check]                             |
|                                                                                  |
|                                                [Back]              [Continue]     |
+----------------------------------------------------------------------------------+
```

### B) Main Desktop Runtime UI (Live Mode)
```text
+------------------------------------------------------------------------------------------------+
| Recordit                                               Status: RECORDING  00:03:41      [Stop] |
+------------------------------------------------------------------------------------------------+
| Mode: (•) Live Transcribe   ( ) Record Only     Language: English     Quality: Balanced       |
|------------------------------------------------------------------------------------------------|
| Transcript                                                                                     |
| [00:00.000-00:00.420] mic: hello everyone                                                     |
| [00:00.050-00:00.410] system: welcome to the meeting                                          |
| [00:02.100-00:02.780] mic ~ we will start in a minute...                                      |
| [00:02.900-00:03.200] mic: we are now live                                                    |
|                                                                                                |
|----------------------------------------------------------------------------------------------- |
| Session Health                                                                                |
| Trust: OK      Degradation Notices: 0      Queue: Normal                                      |
|                                                                                                |
| [Open Session Folder]  [Open Manifest]  [Copy Transcript]                                     |
+------------------------------------------------------------------------------------------------+
```

## Interaction Rules
1. `Start` is disabled until required permissions are granted.
2. `Live Transcribe` is disabled when no model is available.
3. If model missing and offline, app auto-switches recommendation to `Record Only`.
4. `Stop` always triggers graceful finalize before UI returns to idle.
5. Transcript panel only shows stable lines in non-live rendering contexts.
6. Runtime status must remain visible even when transcript is empty.
7. Record-only sessions must expose a clear "Transcribe now" action after completion.

## UX Copy Rules (General Users)
- Prefer plain language:
  - "Needs permission" instead of "TCC denied."
  - "Transcript quality reduced" instead of raw degradation codes.
- Keep technical details in an optional "Details" drawer.

## Error and Recovery UX
1. Permission denied:
   - show exact missing permission
   - provide one-click route to System Settings
2. Model download failed:
   - preserve progress if possible
   - provide retry and network guidance
3. Runtime failure:
   - keep partial artifacts
   - show clear next action (retry, switch mode, open details)

## Acceptance Criteria (UX)
1. New user can complete install + first recording without Terminal.
2. Onboarding detects and verifies both permissions in-app.
3. Live mode shows transcript updates during active runtime.
4. User can manually stop session and get finalized output.
5. Offline first-run still allows recording with explicit deferred transcription flow.
6. Session end always presents clear status and artifact access actions.

## Engineering Acceptance Criteria (Technical)
1. UI can run 10 consecutive live sessions without app restart and without orphaned subprocesses.
2. For each completed session, `session.manifest.json` exists and parses successfully.
3. JSONL tailer handles append cadence without duplicate line rendering.
4. Degraded sessions surface trust/degradation info in UI without being marked failed.
5. Failed sessions include at least one actionable recovery path in UI.
6. Offline first-run with no model still permits record-only capture + artifact generation.

## Test Plan

### Unit Tests
1. Session state machine transitions and invalid transition rejection.
2. JSONL event decoding and unknown-event tolerance.
3. Manifest status mapping (`OK` / `Degraded` / `Failed`).
4. Model checksum verification and atomic publish logic.

### Integration Tests
1. Process manager start/stop + timeout behavior.
2. Live run emits transcript before shutdown.
3. Record-only + deferred offline transcription pipeline.
4. Permission denied path prevents live start and surfaces remediation.

### UI Automation (XCUITest)
1. First-run onboarding happy path.
2. Permission denied then recovered.
3. Model download interruption and retry.
4. Main screen live run with transcript updates.
5. Session summary with artifact open actions.

## Rollout Plan (Technical)
1. Milestone A: onboarding + model manager + process manager scaffolding.
2. Milestone B: main session view + JSONL tail + manifest finalization surface.
3. Milestone C: deferred transcription UX + reliability hardening + packaging polish.
4. Milestone D: acceptance gates + release checklist + docs handoff.

---

## Appendix: Session Library Extension (Approved)

This appendix appends and extends the v1 plan with previous-session access.

### Scope Adjustment
1. In-scope addition:
   - users can access previous sessions in-app
   - users can play session audio
   - users can read recorded conversation transcript lines
   - users can export and delete sessions
2. This is an essentials library scope, not a full knowledge-management product.
3. Still out-of-scope:
   - tags/favorites
   - cloud sync
   - multi-root indexing

### Session Library UX
1. Add top-level navigation item: `Sessions`.
2. `Sessions` list supports:
   - newest-first default sort
   - mode filter (`Live`, `Record Only`)
   - status filter (`Pending`, `OK`, `Degraded`, `Failed`)
   - text search on session title/timestamp + transcript content (when indexed)
3. Session detail screen supports:
   - audio playback of canonical `session.wav`
   - transcript/conversation timeline rendering
   - trust/degradation summary in plain language
   - actions: open folder, export transcript, export bundle, delete

### Session Source and Storage Policy
1. Data source for library:
   - app-managed session roots only
   - canonical: `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/sessions/<YYYYMMDD>/<timestamp>-<mode>/`
2. Required files for a valid session item:
   - `session.manifest.json`
   - `session.wav` (playback-enabled)
3. Optional files:
   - `session.jsonl`
   - `session.input.wav`
4. Pending record-only sessions:
   - valid pending item when `session.wav` + `session.pending.json` exists, even if manifest is absent
   - UI status for these items is `Pending`
5. Migration/compatibility ingest:
   - app may ingest legacy flat packaged artifacts (`<root>/<session-stem>.*`) as import-only records
   - new sessions must be normalized into unique per-session roots
6. Retention default:
   - keep until user deletes
   - no auto-purge in this phase

### Transcript / Conversation Resolution
1. Preferred source:
   - `session.manifest.json` transcript/events surfaces.
2. Fallback source:
   - `session.jsonl` stable transcript events (`final`, `llm_final`, `reconciled_final`).
3. Deterministic ordering key:
   - `start_ms`, `end_ms`, `event_type_rank`, `channel`, `segment_id`, `source_final_segment_id`, `text`
   - `event_type_rank`: `partial < final < reconciled_final < llm_final`
4. If any `reconciled_final` exists for display scope, canonical transcript should prefer reconciled lines over raw `final`.
5. If transcript content is unavailable:
   - session stays accessible with metadata/audio where available
   - conversation panel shows a graceful empty state

### Technical Components (Additive)
1. `SessionLibraryService`
   - scans and indexes app-managed sessions
2. `SessionDetailResolver`
   - resolves metadata, transcript, audio availability, trust summary
3. `SessionExportService`
   - exports transcript/audio/session bundle
4. `SessionDeletionService`
   - deletes by moving session root to Trash

### Additional UI Data Models
1. `UiSessionHistoryItem`
2. `UiSessionDetail`
3. `UiSessionConversationLine`
4. `UiSessionFilter`
5. `UiSessionExportRequest`

### Session Library ASCII Art

#### Sessions List
```text
+--------------------------------------------------------------------------------------------------+
| Sessions                                                     Search: [ team sync            ]      |
+--------------------------------------------------------------------------------------------------+
| Filters: [All] [Live] [Record Only] [Pending] [OK] [Degraded] [Failed] Sort: Newest            |
|--------------------------------------------------------------------------------------------------|
| 2026-03-04 14:32  Live Transcribe   Degraded   00:18:20   [Open] [Export] [Delete]             |
| 2026-03-04 09:10  Record Only       OK         00:07:42   [Open] [Export] [Delete]             |
| 2026-03-03 17:55  Live Transcribe   OK         00:22:01   [Open] [Export] [Delete]             |
+--------------------------------------------------------------------------------------------------+
```

#### Session Detail (Audio + Conversation)
```text
+--------------------------------------------------------------------------------------------------+
| Session: 2026-03-04 14:32                              Status: Degraded                  [Back]  |
+--------------------------------------------------------------------------------------------------+
| Audio Playback                                                                   [Play] [Pause] |
| [=====================|--------------------------------------------------------]     00:03/18:20 |
|--------------------------------------------------------------------------------------------------|
| Recorded Conversation                                                                            |
| [00:00.000-00:00.420] mic: hello everyone                                                       |
| [00:00.050-00:00.410] system: welcome to the meeting                                            |
| [00:02.900-00:03.200] mic: we are now live                                                      |
|                                                                                                  |
| Trust Summary: Transcript quality reduced under queue pressure.                                  |
|                                                                                                  |
| [Open Folder] [Export Transcript] [Export Bundle] [Delete Session]                              |
+--------------------------------------------------------------------------------------------------+
```

### Extended Acceptance Criteria
1. User can access previous sessions entirely in-app.
2. User can play `session.wav` for a selected previous session.
3. User can view recorded conversation lines for previous sessions.
4. User can export transcript/audio/bundle from session detail.
5. User can delete a session via confirmation flow and see list refresh.

### Deferred Record-Only Queue Contract
1. Record-only sessions must write a sidecar descriptor:
   - `<session_root>/session.pending.json`
2. Required fields:
   - `session_id`
   - `created_at_utc`
   - `wav_path`
   - `mode` (`record_only`)
   - `transcription_state`
3. `transcription_state` enum:
   - `pending_model`, `ready_to_transcribe`, `transcribing`, `completed`, `failed`
4. `Transcribe pending session` UI action targets `ready_to_transcribe`.
5. On successful deferred transcription:
   - remove `session.pending.json`
   - treat `session.manifest.json` as canonical completion and trust/degradation source.

---

## Appendix: Authoritative References + Open Decisions

Last reviewed: 2026-03-04

### A) External Authoritative References (Apple)
1. Distribution outside Mac App Store:
   - https://help.apple.com/xcode/mac/current/en.lproj/dev033e997ca.html
2. Signing and distributing apps:
   - https://help.apple.com/xcode/mac/current/en.lproj/devf87a2ac8f.html
3. Notarization:
   - https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
4. App Sandbox:
   - https://developer.apple.com/documentation/security/app-sandbox
   - https://developer.apple.com/documentation/xcode/configuring-the-macos-app-sandbox/
5. Screen capture permission APIs:
   - https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess()
   - https://developer.apple.com/documentation/coregraphics/cgrequestscreencaptureaccess()
6. Microphone permission and usage description:
   - https://developer.apple.com/documentation/bundleresources/information-property-list/nsmicrophoneusagedescription
   - https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media
7. SwiftUI data flow:
   - https://developer.apple.com/documentation/swiftui/model-data
   - https://developer.apple.com/videos/play/wwdc2019/226/
8. Localization (string catalogs):
   - https://developer.apple.com/documentation/xcode/localizing-and-varying-text-with-a-string-catalog
9. Trash-based deletion API:
   - https://developer.apple.com/documentation/foundation/filemanager/trashitem(at:resultingitemurl:)

### B) Repository-Authoritative References (Implementation Contracts)
1. Packaged path and permission behavior:
   - `README.md` sections: packaging/signing, output paths, permissions.
2. Capture contract and argument semantics:
   - `README.md` (`sequoia_capture` arguments)
   - `docs/state-machine.md` (Shared Capture Runtime State Machine).
3. Runtime/preflight artifact schema:
   - `contracts/session-manifest.schema.v1.json`
   - `contracts/runtime-jsonl.schema.v1.json`
   - `docs/session-manifest-schema.md`
   - `docs/live-jsonl-event-contract.md`
4. Packaged entrypoint policy:
   - `docs/adr-004-packaged-entrypoint.md`
5. Entitlements and plist keys in repository:
   - `packaging/Info.plist`
   - `packaging/entitlements.plist`

### C) Decision Log (Resolved)
1. Distribution lane for v1 desktop:
   - Decision: release DMG artifacts via GitHub Releases as the canonical distribution channel.
   - Release gate policy:
     - v1 beta: GitHub Release DMG is required; signing/notarization are strongly recommended and tracked in release checklist.
     - v1 GA target: `Developer ID signed + notarized DMG` required.
2. App-shell architecture convention:
   - Decision: `MVVM + Coordinator + Service layer` (no direct process IO in Views).
3. Retention/deletion policy:
   - Decision: `keep-until-user-delete`; delete uses Trash; no auto purge in v1.
4. Storage roots as product policy:
   - Decision: lock production storage to app container roots below.
   - Canonical roots:
     - sessions root: `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/sessions/`
     - models root: `~/Library/Containers/com.recordit.sequoiatranscribe/Data/models/`
     - logs root: `~/Library/Containers/com.recordit.sequoiatranscribe/Data/logs/`
   - Session directory shape:
     - `.../sessions/<YYYYMMDD>/<timestamp>-<mode>/`
     - required outputs on completion: `session.wav`, `session.manifest.json`
     - optional: `session.jsonl`, `session.input.wav`, `session.pending.json`
   - Indexing policy:
     - session library indexes only the canonical sessions root
     - legacy flat artifacts (`<root>/<session-stem>.*`) can be ingested as migration/import records
   - Write policy:
     - write to temp path first then atomic rename for manifest/pending sidecar
     - never write outside container roots from app-managed flows
   - UI path policy:
     - UI may show shortened paths by default but must preserve full absolute path in details/copy actions.
5. Permission recovery UX policy:
   - Decision: always show "You may need to quit and reopen Recordit" after screen recording permission changes, with `Re-check` action.
6. Localization scope for v1:
   - Decision: English shipped first; app remains string-catalog-ready from day one.
7. Export/privacy defaults:
   - Decision: diagnostics exports exclude transcript text by default; transcript-bearing diagnostics require explicit opt-in.
   - Export types:
     - `Export Transcript` (user-intent content export): includes transcript text.
     - `Export Audio` (when provided): includes `session.wav`.
     - `Export Bundle`: includes canonical session artifacts for portability/review.
     - `Export Diagnostics`: for troubleshooting, privacy-minimized by default.
   - Diagnostics default contents:
     - include: `session.manifest.json`, runtime metadata, counters, statuses, trust/degradation signals.
     - include: JSONL with transcript text fields redacted when transcript opt-in is off.
     - exclude by default: raw transcript lines, transcript text in JSONL events, full audio payloads.
   - Diagnostics opt-in behavior:
     - explicit checkbox: `Include transcript text in diagnostics` (default off).
     - optional checkbox: `Include audio in diagnostics` (default off).
     - confirmation copy must warn user this may include sensitive conversation content.
   - File naming:
     - transcript export: `recordit-transcript-<session_id>.txt` (or `.md`)
     - bundle export: `recordit-session-<session_id>.zip`
     - diagnostics export: `recordit-diagnostics-<session_id>.zip`

### D) Closure Criteria
This appendix is currently resolved for v1 scope. Any future change to section C decisions requires an explicit ADR/release decision note and a corresponding update to this UX spec.

### E) Execution Backlog (Beads Index)
Canonical implementation graph has been created in Beads and is the execution source of truth.

1. Root delivery epic:
   - `bd-exba` - Desktop UX/UI v1 delivery graph (contract-aligned, self-documenting)
2. Workstream epics:
   - `bd-gmbg` - Desktop app shell architecture and module boundaries
   - `bd-25kd` - Onboarding + preflight gating + permission recovery UX
   - `bd-2c1s` - Session runtime orchestration and live transcript surface
   - `bd-2n0s` - Session library/history UX (audio + conversation access)
   - `bd-8qjl` - Deferred record-only transcription queue
   - `bd-2vuj` - Export surfaces and privacy-preserving diagnostics
   - `bd-euj6` - Storage policy, atomic artifact writes, and migration integrity
   - `bd-2zhv` - QA automation, contract verification, and reliability gates
   - `bd-twiz` - Release operations for GitHub DMG channel (beta->GA hardening)
3. Graph status at creation time:
   - no dependency cycles
   - parent-child hierarchy plus explicit `blocks` dependencies across workstreams
   - ready queue starts with architecture/storage foundations to maximize parallel safe execution
4. Post-audit user-centric optimizations added:
   - `bd-3lkr` - Accessibility and keyboard-first UX hardening (sessions/runtime/onboarding)
   - `bd-fggr` - Interruption recovery UX: session resume and partial artifact salvage
   - `bd-125h` - Deferred transcription notifications and actionable completion states
   - `bd-2iuz` - Time-to-first-transcript budget and UX responsiveness guardrails
5. Second-round plan-space optimizations added:
   - `bd-2m6f` - UX copy lexicon + String Catalog scaffold (English-first, localization-ready)
   - `bd-3oc4` - Incremental transcript indexing for fast session search
   - `bd-2jsx` - Startup runtime binary readiness checks and remediation UX
   - `bd-3i9w` - Enforce backend capability matrix in model/setup UI
6. Third-round execution ergonomics improvements:
   - umbrella roadmap issues (`bd-exba` + workstream features) are deferred for 30 days to keep `br ready` focused on implementation tasks
   - all currently open implementation beads now carry specific (non-generic) acceptance criteria and intent notes
