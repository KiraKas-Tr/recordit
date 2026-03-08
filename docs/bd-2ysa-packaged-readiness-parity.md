# bd-2ysa — Packaged Readiness Parity + Record Only Fallback Evidence

Bead: `bd-2ysa`

## Purpose

Add one deterministic packaged-app readiness parity lane that verifies canonical readiness scenarios against packaged `Recordit.app` runtime behavior and records contract-aligned gating decisions.

This closes the gap where readiness/fallback parity was primarily shown in dev/XCUITest seams.

## Entry Point

```bash
scripts/gate_packaged_readiness_parity.sh
```

Useful options:

```bash
scripts/gate_packaged_readiness_parity.sh --skip-build
scripts/gate_packaged_readiness_parity.sh --out-dir artifacts/validation/bd-2ysa/manual-run --skip-build
```

## Required Scenarios

1. `missing-permission`
2. `no-display`
3. `runtime-preflight-failure`
4. `fully-ready`
5. `live-blocked-record-allowed-fallback`

The summary gate fails if any required scenario row is missing.

## What Is Verified Per Scenario

For each scenario, the lane retains:

- scenario inputs (`scenario_meta.json`, `execution.json`)
- preflight payload (`preflight.manifest.json` for real or synthetic fixture cases)
- mapped readiness IDs + domains/policies
- blocking vs warning classification
- final gating decisions:
  - `can_proceed_without_ack`
  - `can_proceed_with_ack`
  - `record_only_fallback_eligible`
  - `primary_blocking_domain`

Mapping semantics are contract-driven from `contracts/readiness-contract-ids.v1.json`, matching `PreflightGatingPolicy` behavior.

## Scenario Execution Model

- `fully-ready`: real packaged runtime preflight invocation.
- `runtime-preflight-failure`: real packaged runtime invocation with deterministic output-path blocker (`session.wav` pre-created as directory).
- `live-blocked-record-allowed-fallback`: real packaged runtime invocation from isolated workdir/home so model resolution fails in backend-model domain, proving Record Only fallback eligibility.
- `missing-permission`, `no-display`: synthetic preflight fixtures for host constraints that cannot be forced deterministically in CI/local automation.

## Retained Outputs

For run root `<out-dir>`:

- `<out-dir>/artifacts/readiness_parity_matrix.csv`
- `<out-dir>/artifacts/readiness_parity_matrix.json`
- `<out-dir>/artifacts/readiness_parity_matrix_status.txt`
- `<out-dir>/artifacts/readiness_parity_matrix_status.json`
- `<out-dir>/scenarios/<scenario-id>/scenario_meta.json`
- `<out-dir>/scenarios/<scenario-id>/execution.json`
- `<out-dir>/scenarios/<scenario-id>/stdout.log`
- `<out-dir>/scenarios/<scenario-id>/stderr.log`

Shared evidence-contract root files are also emitted:

- `<out-dir>/evidence_contract.json`
- `<out-dir>/summary.csv`
- `<out-dir>/summary.json`
- `<out-dir>/status.txt`
- `<out-dir>/paths.env`

## Validation Commands

```bash
bash -n scripts/gate_packaged_readiness_parity.sh
python3 -m py_compile scripts/gate_packaged_readiness_parity_summary.py
python3 -m unittest scripts/test_gate_packaged_readiness_parity_summary.py
```
