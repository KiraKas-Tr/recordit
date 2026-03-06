# Runtime Boundary Ownership Contract (v1)

Date: 2026-03-06  
Primary bead: `bd-290k`  
Related beads: `bd-20r2`, `bd-2r6k`

## Purpose

Prevent SwiftUI↔Rust boundary drift by defining one explicit ownership contract for:

- runtime truth
- readiness truth
- lifecycle truth
- user remediation/orchestration behavior

This document is normative for app/runtime boundary decisions unless a newer contract explicitly supersedes it.

## Scope

Applies to:

- `recordit` operator shell and `transcribe-live` runtime/preflight surfaces
- SwiftUI app shell (`Recordit.app`) onboarding/session/recovery decisions
- runtime artifact interpretation (JSONL + manifest + preflight manifest)

Does not redefine lower-level audio DSP/callback internals. Those remain governed by runtime contracts and architecture docs.

## Ownership Matrix

| Surface | Owner | Responsibility | Must Not Own |
|---|---|---|---|
| Runtime execution truth (`run`, lifecycle, transcript emission, closeout) | Rust runtime | Capture/transcribe execution, lifecycle transitions, artifact emission, degradation/trust signaling | SwiftUI-only UX routing policy |
| Preflight readiness truth (check IDs + pass/warn/fail values) | Rust preflight | Authoritative readiness diagnostics (`model_path`, `screen_capture_access`, `display_availability`, etc.) and remediation details | View navigation state or button enablement rules |
| Startup binary readiness (recordit/sequoia_capture availability) | Swift RuntimeProcessLayer + runtime binary resolver | Resolve executable paths, classify missing/not-executable/invalid override, expose `runtimeUnavailable` failure surface | Re-implementing Rust preflight check IDs |
| UX orchestration (onboarding steps, affordances, recovery routing, fallback messaging) | SwiftUI app shell/coordinators | Decide what the user sees/does next based on contract outputs | Recomputing runtime/preflight truth independently |
| Artifact schema and mode taxonomy contracts | Rust + `contracts/*.json` | Versioned machine-readable schema/contract files and compatibility rules | App-local undocumented schema forks |

## Boundary Invariants

1. Rust preflight output is the single source of truth for live-readiness diagnostics.
2. SwiftUI must consume preflight/runtime contract IDs and statuses; it must not invent alternate readiness definitions with overlapping semantics.
3. Startup binary readiness and live runtime readiness are distinct lanes:
   - startup binary readiness answers "can app invoke required binaries?"
   - preflight/runtime readiness answers "can live capture/transcribe run correctly now?"
4. Runtime degradation must stay explicit via runtime events/manifests (`trust_notice`, `degradation_events`) rather than being hidden by UX.
5. Record Only remains a first-class fallback when live readiness is blocked but recording remains viable.
6. Runtime/public contract files under `contracts/` are versioned compatibility boundaries; breaking changes require explicit version bump + migration docs.

## Source Mapping (Implementation Anchors)

Rust/runtime side:

- `src/recordit_cli.rs`
- `src/bin/transcribe_live/app.rs`
- `src/bin/transcribe_live/preflight.rs`
- `contracts/runtime-mode-matrix.v1.json`
- `contracts/runtime-jsonl.schema.v1.json`
- `contracts/session-manifest.schema.v1.json`
- `contracts/recordit-exit-code-contract.v1.json`

Swift/app side:

- `app/RuntimeProcessLayer/RuntimeProcessManager.swift`
- `app/RuntimeProcessLayer/RuntimeBinaryReadinessService.swift`
- `app/AppShell/PreflightViewModel.swift`
- `app/Preflight/PreflightGatingPolicy.swift`
- `app/AppShell/PermissionRemediationViewModel.swift`
- `app/RecorditApp/MainSessionView.swift`

## Readiness Vocabulary Handshake

The canonical readiness vocabulary is defined by:

1. Preflight check IDs emitted by Rust (`model_path`, `out_wav`, `out_jsonl`, `out_manifest`, `sample_rate`, `screen_capture_access`, `display_availability`, `microphone_access`, `backend_runtime`)
2. Swift gating policy classes (`blockOnFail`, `warnRequiresAcknowledgement`, `informational`)
3. Startup binary readiness statuses (`ready`, `missing`, `not_executable`, `invalid_override`)

Machine-readable baseline:

- `contracts/readiness-contract-ids.v1.json`

## Change Policy

Breaking boundary changes require all of:

1. contract update (`contracts/*.vN.json`) with versioned filename rules
2. this ownership doc update
3. corresponding Swift mapping update
4. contract/e2e evidence updates in tests/docs

If any of the above is missing, the change is incomplete.
