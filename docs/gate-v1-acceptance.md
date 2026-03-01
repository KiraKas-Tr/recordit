# Gate: V1 Acceptance

This gate codifies the operator-visible v1 acceptance bar for near-live mode.
It composes deterministic fake-live cold/warm runs with the existing backlog-pressure gate
so one `summary.csv` answers the core release question without ad hoc interpretation.

## Run

```bash
scripts/gate_v1_acceptance.sh
```

Artifacts are written to:

- `artifacts/bench/gate_v1_acceptance/<timestamp>/cold/`
- `artifacts/bench/gate_v1_acceptance/<timestamp>/warm/`
- `artifacts/bench/gate_v1_acceptance/<timestamp>/backlog_pressure/`
- `artifacts/bench/gate_v1_acceptance/<timestamp>/summary.csv`
- `artifacts/bench/gate_v1_acceptance/<timestamp>/status.txt`

## Acceptance Bar

`summary.csv` publishes the following booleans and overall `gate_pass`.

Cold/warm live behavior:

1. `cold_first_emit_during_active_ok=true`
2. `warm_first_emit_during_active_ok=true`
3. `cold_artifact_truth_ok=true`
4. `warm_artifact_truth_ok=true`

Pressure/trust behavior:

5. `backlog_pressure_thresholds_ok=true`
6. `backlog_degradation_signal_ok=true`
7. `backlog_trust_signal_ok=true`
8. `backlog_surface_ok=true`
9. `backlog_gate_pass=true`

Interpretation:

- cold/warm first-emit checks are based on JSONL event ordering, not terminal scraping:
  the first transcript event must occur after lifecycle `active` and before `draining`
- artifact truth checks require manifest `out_wav_materialized=true`, non-zero `out_wav_bytes`,
  and a materialized output file at the manifest path
- backlog pressure checks are delegated to `scripts/gate_backlog_pressure.sh`, which already
  proves non-blocking degradation and trust/degradation surfacing under induced pressure

This keeps the v1 decision grounded in deterministic machine-readable evidence rather than
manual log inspection.
