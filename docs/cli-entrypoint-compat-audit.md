# CLI/Entrypoint Compatibility Audit (bd-v027)

Date: 2026-03-02
Scope: Makefile targets, helper scripts, and packaged entrypoints impacted by the `recordit` CLI rollout.

## Decision Classes

- `legacy-stable`: keep on `transcribe-live`/packaged binary for compatibility in this cycle.
- `wrap-with-recordit`: operator-facing wrapper should call `recordit` instead of raw legacy flags.
- `untouched-this-cycle`: not part of CLI migration risk surface for this bead.

## Entry Point Inventory and Classification

| Surface | Location | Current invocation family | Class | Decision for this cycle | Rationale |
|---|---|---|---|---|---|
| Canonical operator binary | `src/main.rs` | `recordit_cli::main()` | `wrap-with-recordit` | keep as canonical | This is already the human-first CLI shell; no migration needed. |
| Legacy compatibility binary | `src/bin/transcribe_live.rs` | `app::main()` | `legacy-stable` | keep stable/additive | Help text now points operators to `recordit`, while preserving automation/gate behavior. |
| Debug runtime targets | `Makefile`: `transcribe-live`, `transcribe-live-stream`, `transcribe-preflight`, `transcribe-model-doctor`, `capture-transcribe` | `cargo run --bin transcribe-live ...` | `legacy-stable` | keep unchanged | Existing gate/debug workflows depend on legacy selectors and explicit artifact flags. |
| Smoke targets | `Makefile`: `smoke-offline`, `smoke-near-live`, `smoke-near-live-deterministic` | `cargo run --bin transcribe-live ...` | `legacy-stable` | keep unchanged | Deterministic smoke baselines and mode taxonomy checks are pinned to legacy contract surfaces. |
| Compatibility gate wrappers | `Makefile`: `gate-backlog-pressure`, `gate-transcript-completeness`, `gate-v1-acceptance`, `gate-d-soak` | `scripts/gate_*.sh` (backed by `transcribe-live`) | `legacy-stable` | keep unchanged | These are compatibility gates, not operator UX wrappers; moving now would invalidate frozen evidence paths. |
| Packaged smoke gate | `Makefile gate-packaged-live-smoke` + `scripts/gate_packaged_live_smoke.sh` | signed `SequoiaTranscribe.app` binary | `legacy-stable` | keep unchanged | Packaged evidence currently validates signed app behavior against the legacy runtime contract family. |
| Packaged run wrappers | `Makefile`: `run-transcribe-app`, `run-transcribe-live-stream-app`, `run-transcribe-preflight-app`, `run-transcribe-model-doctor-app` | signed `SequoiaTranscribe.app` (`transcribe-live`) | `legacy-stable` | keep unchanged | ADR-backed packaged entrypoint policy remains on `SequoiaTranscribe` in this cycle. |
| Contract/schema enforcement wrapper | `Makefile contracts-ci` + `scripts/ci_contracts.sh` | contract-focused `cargo test` suite including `recordit_*` tests | `wrap-with-recordit` | keep as-is | This path already validates `recordit` machine contract surfaces and schema publication. |
| Capture-only engineering paths | `Makefile`: `probe`, `capture`; `scripts/mixed_rate_regression.sh`; `scripts/setup_whispercpp_model.sh` | `sequoia_capture` / setup tooling | `untouched-this-cycle` | no migration action | These do not consume the `recordit` command grammar and are outside the migration boundary. |

## Script-Level Notes (Audit Highlights)

1. `scripts/gate_backlog_pressure.sh`, `scripts/gate_v1_acceptance.sh`, `scripts/gate_d_soak.sh`, and `scripts/gate_transcript_completeness.sh` build/execute `target/debug/transcribe-live` directly.
2. `scripts/gate_packaged_live_smoke.sh` validates the signed packaged executable `dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe`.
3. `scripts/ci_contracts.sh` is the current CI-oriented machine-contract enforcement surface and already covers `recordit` contract behavior tests.

## Outcome for Phase G Follow-Ons

- `bd-v027` migration boundary decision: compatibility gates and packaged wrappers remain on legacy entrypoints this cycle.
- New operator-first wrappers added after this audit should target `recordit` grammar (`run --mode live|offline`, `doctor`, `preflight`, `replay`, `inspect-contract`) instead of introducing new `transcribe-live` wrappers.
- Downstream compatibility beads (`bd-mcva`, `bd-3f6g`) can treat this matrix as the source for what must stay stable while comparing outputs against frozen baselines.
