# Representative-Chunked Deprecation Go/No-Go Review (bd-119)

Date: 2026-03-01
Status: in-progress (`HOLD`) pending packaged evidence from `bd-3dx`
Owner: `HazyDune`

## Purpose

Decide whether to deprecate or retain representative chunk mode (`--live-chunked`)
after CLI v1 and packaged follow-on evidence are both available.

This review exists to prevent premature selector deprecation while preserving the
current compatibility contract:

- `--live-stream` is the true concurrent capture+transcribe path
- `--live-chunked` remains representative-chunked validation with compatibility labels

References:

- `docs/live-chunked-migration.md`
- `docs/packaged-live-stream-plumbing.md`
- `docs/gate-v1-acceptance.md`

## Current Decision State

`HOLD` (no-go-by-default until required packaged evidence is complete).

Rationale:

1. CLI-side evidence is available (`bd-nqn`/v1 acceptance gate landed).
2. Packaged diagnostics parity lane (`bd-3ma`) is complete.
3. Packaged smoke/gate evidence lane (`bd-3dx`) is still in progress and is a hard input.

## Required Evidence Inputs

All inputs below must be present before finalizing go/no-go.

### A) CLI v1 Baseline (already available)

- `artifacts/bench/gate_v1_acceptance/<timestamp>/summary.csv`
- `gate_pass=true` with first-emit/artifact/trust checks passing

Current local evidence snapshot:

- `artifacts/tmp/gate_v1_acceptance_fake/summary.csv`
  - `gate_pass=true`
  - `cold_first_emit_during_active_ok=true`
  - `warm_first_emit_during_active_ok=true`
  - `cold_artifact_truth_ok=true`
  - `warm_artifact_truth_ok=true`
  - `backlog_gate_pass=true`
- `artifacts/tmp/gate_backlog_fake/summary.csv`
  - `gate_pass=true`
  - queue pressure/trust/reconciliation thresholds all `true`
- `artifacts/tmp/gate_d_fake/summary.csv`
  - `gate_pass=true`
  - harness reliability / latency drift / continuity visibility thresholds all `true`

### B) Packaged Diagnostics Parity (already available)

- `bd-3ma` closure evidence:
  - packaged live diagnostics invocation path documented
  - parser/test coverage proving `--live-stream --model-doctor` compatibility

### C) Packaged Live Smoke/Gate Evidence (pending `bd-3dx`)

Required once available:

- packaged gate summary artifact(s) under packaged container-root evidence path
- manifest proof from signed-app context with `runtime_mode=live-stream`
- trust/degradation surface consistency under packaged path

Expected artifact shape once `bd-3dx` lands:

- `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/gates/<gate>/<timestamp>/summary.csv`
- `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/gates/<gate>/<timestamp>/status.txt`
- packaged runtime manifest(s) proving `runtime_mode=live-stream`

### D) Operator Messaging Readiness (partially available)

- migration guidance remains explicit:
  - `--live-stream` = true live
  - `--live-chunked` = representative compatibility path
- no doc ambiguity that interprets `runtime_mode=live-chunked` as true live proof

## Go Criteria

Recommend `GO` only if all are true:

1. CLI v1 acceptance gate passes deterministically.
2. Packaged diagnostics parity is validated and documented.
3. Packaged smoke/gate evidence passes with machine-readable artifacts.
4. No contract-level regressions in replay/schema compatibility.
5. Operator docs clearly preserve migration semantics during deprecation rollout.

## No-Go / Hold Criteria

Hold or reject deprecation if any are true:

1. Packaged smoke/gate evidence is missing or ambiguous.
2. Packaged live artifacts fail to prove `runtime_mode=live-stream`.
3. Trust/degradation signaling diverges between CLI and packaged paths.
4. Tooling still depends on `runtime_mode=live-chunked` as a true-live proxy.

## Planned Final Decision Template

Fill this section once `bd-3dx` is complete.

- Final recommendation: `GO` | `NO-GO` | `HOLD`
- Effective date:
- Scope:
  - selector lifecycle intent
  - compatibility grace period
  - rollback trigger
- Evidence snapshot:
  - CLI acceptance artifact path(s)
  - packaged gate artifact path(s)
  - supporting docs/contracts updated

## Decision Procedure (When `bd-3dx` Artifacts Arrive)

1. Confirm packaged gate pass signal from the new packaged summary artifact.
2. Confirm packaged manifest reports `runtime_mode=live-stream`.
3. Confirm trust/degradation surfaces are present and non-empty when pressure is induced.
4. Re-check migration messaging consistency in:
   - `docs/live-chunked-migration.md`
   - `README.md`
   - `docs/realtime-contracts.md`
5. Write final recommendation in this file and close `bd-119`.

Suggested command pattern (fill concrete packaged paths from `bd-3dx` closeout):

```bash
cat <packaged_gate_summary.csv>
jq '.runtime_mode, .runtime_mode_taxonomy, .trust, .degradation_events' <packaged_runtime_manifest.json>
```

## Immediate Next Step

Wait for `bd-3dx` completion artifacts, then update this document with final
decision and close `bd-119`.
