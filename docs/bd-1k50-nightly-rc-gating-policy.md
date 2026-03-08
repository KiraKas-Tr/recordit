# bd-1k50 — Nightly and RC Gating Policy for Comprehensive Real-Environment Verification

Date: 2026-03-07  
Related bead: `bd-1k50`  
Primary suite entrypoint: `scripts/gate_comprehensive_real_environment_suite.sh`  
Primary runbook: `docs/bd-2t10-comprehensive-real-environment-runbook.md`

## Purpose

Define one policy for how the comprehensive suite is executed and interpreted in:

1. nightly regression posture, and
2. release-candidate (RC) signoff posture.

This policy is intentionally strict about wording and release decisions: simulation-rich evidence can be useful, but it cannot be treated as real-environment proof when RC gates require real execution.

## Canonical Execution Profiles

## Nightly profile

Preferred command:

```bash
scripts/gate_comprehensive_real_environment_suite.sh \
  --allow-capability-gated \
  --out-dir artifacts/ops/gate_comprehensive_real_environment_suite/nightly/<stamp>
```

Nightly may use capability-gated execution when host constraints are explicit and retained.

## RC profile

Required command:

```bash
scripts/gate_comprehensive_real_environment_suite.sh \
  --out-dir artifacts/ops/gate_comprehensive_real_environment_suite/rc/<rc-tag>
```

RC profile must not use `--allow-capability-gated`, `--dry-run`, or skip-phase flags for required lanes.

## Lane Tier Policy

The comprehensive suite currently orchestrates these phases:

- `preflight_capability_checks`
- `default_user_journey`
- `packaged_stop_taxonomy`
- `release_context_verification`
- `anti_bypass_guard`
- `mock_exception_guard`
- `suite_summary_checks`

Policy tiering:

| Phase | Nightly | RC | Notes |
| --- | --- | --- | --- |
| `preflight_capability_checks` | required | required | capability posture must always be retained |
| `default_user_journey` | required | required | canonical app journey proof |
| `packaged_stop_taxonomy` | required | required | stop/finalization class stability |
| `release_context_verification` | required-if-capable | required | nightly may warn/skip only with explicit capability note; RC cannot |
| `anti_bypass_guard` | required | required | blocks seam-heavy over-claims |
| `mock_exception_guard` | required | required | enforces exception-register discipline |
| `suite_summary_checks` | required when non-gated | required | RC requires strict pass with no required skip/fail |

## Retention Requirements

Every nightly and RC run must retain:

1. suite root contract files:
   - `evidence_contract.json`
   - `summary.csv`
   - `summary.json`
   - `status.txt`
   - `status.json`
   - `paths.env`
2. suite artifacts:
   - `artifacts/preconditions.json`
   - `artifacts/phases.json`
   - `artifacts/suite_checks.csv`
   - `artifacts/suite_checks.json`
3. lane-local roots referenced in `paths.env` and phase notes.

Retention SLO:

- nightly: retain full roots for at least 14 days; retain summary/status surfaces for at least 30 days.
- RC: retain full roots for the entire RC lifecycle and final release audit window (minimum 90 days).

## Rerun Policy

## Nightly rerun policy

1. If any required phase fails with `product_failure`, allow one immediate rerun.
2. If rerun also fails, mark nightly red and open/update a follow-up bead with the retained root.
3. If failure is `contract_failure`, do not downgrade; treat as pipeline integrity incident and escalate immediately.
4. If run is capability-gated, do not mark as full-pass; mark as `warn` posture and track missing capability debt.

## RC rerun policy

1. Required-phase failure blocks RC signoff immediately.
2. One rerun is allowed only after a concrete fix or environment correction; no blind reruns.
3. `contract_failure` always blocks RC.
4. Required-phase `skipped` always blocks RC.

## Release-Blocking Rules

RC is blocked if any of the following is true:

1. suite `status.txt` reports `status=fail`,
2. `artifacts/suite_checks.json` has any `required_failures`,
3. `artifacts/suite_checks.json` has any `required_skipped`,
4. run used capability-gated mode or dry-run mode,
5. required lane-local evidence roots are missing or malformed.

RC may proceed only when:

1. all required phases pass,
2. no required phase is skipped,
3. retained evidence contract validates,
4. strict UI RC evidence gate and release checklist gates are also satisfied.

## Simulation vs Real-Environment Decision Rules

Classification terms:

- `simulation-passed`: lane(s) passed but execution depended on simulation seams, capability gating, or non-RC posture.
- `real-environment-passed`: required lanes passed in strict non-gated posture with retained evidence from real required contexts.

Policy rules:

1. Nightly may report `simulation-passed` for progress tracking.
2. RC signoff cannot use `simulation-passed` as equivalent to real-environment proof.
3. Any certification/release wording must explicitly match the strongest retained evidence class.
4. If evidence is mixed, report mixed posture explicitly (for example: "nightly simulation passed; RC real-environment proof pending").

## Required Reporting Fields

Any nightly/RC summary message or release note that cites this suite must include:

1. suite root path,
2. `status` value,
3. required failure/skip counts,
4. capability-gated flag,
5. whether result is `simulation-passed` or `real-environment-passed`.

## Integration Notes

- `docs/bd-b2qv-release-checklist.md` should treat this policy as authoritative for RC interpretation of comprehensive-suite results.
- Downstream certification gates (for example `bd-2owz`) must consume this policy instead of inventing alternate RC semantics.
