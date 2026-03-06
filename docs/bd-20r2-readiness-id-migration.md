# bd-20r2: Readiness Contract ID Migration Notes

Date: 2026-03-06  
Bead: `bd-20r2`  
Primary contract source: `contracts/readiness-contract-ids.v1.json`

## Purpose

Prevent readiness drift by replacing ad-hoc string IDs with canonical contract IDs across runtime, SwiftUI mapping, remediation logic, and regression smoke tests.

## Canonical ID Sources

- Rust preflight emits canonical IDs from constants in:
  - `src/bin/transcribe_live/preflight.rs`
- SwiftUI consumes canonical IDs from shared constants in:
  - `app/Preflight/PreflightGatingPolicy.swift` (`ReadinessContractID` / `ReadinessContract`)
- Machine-readable baseline remains:
  - `contracts/readiness-contract-ids.v1.json`

## Replacements Completed

1. Rust preflight/model-doctor literal IDs were replaced with constants (`CHECK_ID_*`) and contract-parity tests were added.
2. Swift preflight gating now derives known blocking/warn sets from `ReadinessContract`.
3. Swift remediation and preflight normalization logic now reference `ReadinessContract` IDs rather than hardcoded strings.
4. Readiness-focused smoke fixtures now use `ReadinessContractID` values.

## Migration Rule for Future Changes

When adding or changing readiness IDs:

1. Update `contracts/readiness-contract-ids.v1.json` (or bump version for breaking change).
2. Update shared constants (`ReadinessContractID`/`ReadinessContract` in `PreflightGatingPolicy.swift` and Rust `CHECK_ID_*`).
3. Keep parity tests green:
  - Rust: `preflight_check_ids_match_readiness_contract`
  - Swift smoke: `preflight_gating_smoke`
4. Update user-facing remediation mapping only through canonical IDs (never ad-hoc literals).

## Out of Scope

- This migration does not change readiness product behavior by itself.
- It only hardens the contract vocabulary and drift detection.
