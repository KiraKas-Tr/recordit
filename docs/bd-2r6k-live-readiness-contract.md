# bd-2r6k: SwiftUI↔Rust Live-Readiness Contract

Date: 2026-03-06  
Bead: `bd-2r6k`  
Depends on: `bd-290k`, `bd-20r2`

## Purpose

Define one authoritative readiness payload and shared vocabulary between Rust preflight/runtime and SwiftUI onboarding/gating logic.

This bead locks three requirements:

1. one payload contract for live readiness,
2. explicit readiness lanes (TCC vs runtime preflight vs backend/model),
3. deterministic fallback guidance for Record Only when live-specific blockers are present.

## Authoritative Contract Artifact

- `contracts/readiness-contract-ids.v1.json`

This contract now includes:

- canonical preflight check IDs and classes (`blocking`, `warn_ack_required`)
- canonical readiness domains:
  - `tcc_capture`
  - `backend_model`
  - `runtime_preflight`
  - `backend_runtime`
  - `diagnostic_only`
- authoritative payload lane mapping under `authoritative_live_readiness_payload`
- startup binary readiness statuses shared with Swift runtime readiness (`ready`, `missing`, `not_executable`, `invalid_override`)

## Swift Consumption Surfaces

- `app/Preflight/PreflightGatingPolicy.swift`
  - maps each preflight check into a canonical domain
  - exposes `primaryBlockingDomain`
  - exposes `recordOnlyFallbackEligible`
  - treats diagnostic-only IDs as known contract vocabulary (informational), not unknown drift
- `app/AppShell/PreflightViewModel.swift`
  - surfaces domain/fallback signals to app-shell orchestration
- `app/AppShell/AppShellViewModel.swift`
  - maps domain-aware preflight failures to user-facing error codes:
    - `tcc_capture` -> `permissionDenied`
    - `backend_model` -> `modelUnavailable`
    - `runtime_preflight` / `backend_runtime` -> `preflightFailed`
  - appends Record Only fallback guidance only when blockers are live-specific (`backend_model` / `backend_runtime`) and no other blocking domains are present

## Rust/Contract Drift Guardrails

- `src/bin/transcribe_live/preflight.rs` tests now assert:
  - preflight IDs/classes match contract
  - preflight domain assignments match contract
  - diagnostic-only IDs remain outside live gating IDs

## Validation Targets

- `cargo test --bin transcribe-live preflight::tests`
- `swift app/Preflight/preflight_gating_smoke.swift`
- `swift app/AppShell/onboarding_completion_smoke.swift`
- `make contracts-ci`
