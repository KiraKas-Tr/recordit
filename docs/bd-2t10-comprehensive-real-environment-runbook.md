# bd-2t10 — Master runbook and orchestrator for the comprehensive real-environment suite

Date: 2026-03-07  
Related bead: `bd-2t10`  
Primary orchestrator: `scripts/gate_comprehensive_real_environment_suite.sh`

## Purpose

This runbook defines one deterministic, diagnosable entrypoint for the comprehensive real-environment verification program.

It replaces ad-hoc lane ordering with an explicit sequence that always emits retained evidence, explicit skip reasons, and contract-linked artifacts.

## Canonical command surface

Strict mode (recommended for real signoff):

```bash
scripts/gate_comprehensive_real_environment_suite.sh \
  --out-dir artifacts/ops/gate_comprehensive_real_environment_suite/manual-strict
```

Capability-gated mode (allowed for constrained hosts, but not full signoff):

```bash
scripts/gate_comprehensive_real_environment_suite.sh \
  --allow-capability-gated \
  --out-dir artifacts/ops/gate_comprehensive_real_environment_suite/manual-gated
```

Planning-only dry run (contract + phase topology without executing child lanes):

```bash
scripts/gate_comprehensive_real_environment_suite.sh \
  --dry-run \
  --allow-capability-gated \
  --out-dir artifacts/ops/gate_comprehensive_real_environment_suite/manual-dry-run
```

## Lane sequence (current orchestrated stack)

1. `preflight_capability_checks`
   - captures machine/input posture, TCC assumptions, and capability constraints
2. `default_user_journey`
   - wraps `scripts/gate_default_user_journey_e2e.sh`
3. `packaged_stop_taxonomy`
   - wraps `scripts/gate_packaged_stop_finalization_taxonomy.sh`
4. `release_context_verification`
   - wraps `scripts/verify_recordit_release_context.sh`
5. `anti_bypass_guard`
   - wraps `scripts/gate_anti_bypass_claims.sh`
6. `mock_exception_guard`
   - wraps `scripts/gate_mock_exception_register.sh`
7. `suite_summary_checks`
   - validates required-phase outcomes and writes suite-level checks

## Preconditions (explicit)

The preflight phase records all of the following in `artifacts/preconditions.json`:

- Signing/build posture:
  - `SIGN_IDENTITY`
  - `SKIP_BUILD`
  - command availability (`codesign`, `spctl`, `make`)
- Runtime/model input posture:
  - `RECORDIT_APP_BUNDLE`
  - `RECORDIT_DMG`
  - `MODEL`
  - `FIXTURE`
  - `OFFLINE_INPUT`
  - `PACKAGED_ROOT`
- Host capability posture:
  - macOS requirement (`is_darwin`)
  - required command probes (`xcodebuild`, `hdiutil`, `codesign`, `spctl`)
- Permission/TCC/UI assumptions:
  - logged-in GUI session probe via `launchctl print gui/<uid>`
  - explicit reminder that Screen Recording + Microphone grants are expected for UI-driven phases
- Artifact roots:
  - root output dir + per-lane child output dirs

## Skip semantics (explicit and auditable)

No skip is silent.

Every skipped phase is retained in `artifacts/phases.json` and contract manifests with:

- `status=skipped`
- explicit `exit_classification` (`skip_requested` or `capability_gated`)
- human-readable `notes` with the exact reason

If required capabilities are missing and `--allow-capability-gated` is **not** supplied, preflight records `precondition_failure` and required downstream phases are blocked.

## Expected pass/fail signatures

- **Pass signoff posture**:
  - `status.txt` reports non-fail
  - required phases pass
  - suite summary has no required failures/skips
- **Capability-gated posture**:
  - preflight emits `warn` or explicit `skipped` (dry-run)
  - affected phases are marked non-required skips with reasons
  - output is diagnostically useful but not full-certification proof
- **Hard fail posture**:
  - required phase failure or precondition failure
  - `status.txt` ends in `status=fail`
  - script exits non-zero

## Evidence contract linkage

The orchestrator emits a shared retained-evidence root via `evidence_render_contract` and explicitly links the canonical evidence standards:

- `docs/bd-8ydu-shell-e2e-evidence-contract.md`
- `docs/bd-1ff5-xctest-xcuitest-retained-artifact-contract.md`
- `docs/bd-2j49-cross-lane-e2e-evidence-standard.md`

The generated root always includes:

- `evidence_contract.json`
- `summary.csv` / `summary.json`
- `status.txt` / `status.json`
- `paths.env`
- `artifacts/phases.json`
- `artifacts/preconditions.json`
- `artifacts/suite_checks.csv` and `artifacts/suite_checks.json`

This keeps the suite runnable and diagnosable without tribal knowledge and makes downstream gating (`bd-1k50`) consume one stable orchestration surface.
