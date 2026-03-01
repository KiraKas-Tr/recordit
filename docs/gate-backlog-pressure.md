# Gate: Near-Live Backlog Pressure

This gate codifies near-live queue degradation behavior under intentional pressure.
It is deterministic and host-independent: it runs `transcribe-live --live-chunked`
through the shared fake-capture runtime path (`RECORDIT_FAKE_CAPTURE_FIXTURE`)
with a fixed stereo fixture.

## Run

```bash
scripts/gate_backlog_pressure.sh
```

Optional tuning:

```bash
scripts/gate_backlog_pressure.sh \
  --chunk-window-ms 1200 \
  --chunk-stride-ms 120 \
  --chunk-queue-cap 2 \
  --min-drop-ratio 0.15 \
  --max-drop-ratio 0.80 \
  --min-lag-p95-ms 240
```

Artifacts are written to:

- `artifacts/bench/gate_backlog_pressure/<timestamp>/runtime.manifest.json`
- `artifacts/bench/gate_backlog_pressure/<timestamp>/runtime.jsonl`
- `artifacts/bench/gate_backlog_pressure/<timestamp>/summary.csv`
- `artifacts/bench/gate_backlog_pressure/<timestamp>/status.txt`

## Acceptance Bar

`summary.csv` writes threshold booleans and `gate_pass`.

The scenario must prove pressure is real and signaling is truthful:

1. pressure observed: `dropped_oldest > 0`
2. queue saturation: `high_water >= max_queue`
3. bounded degradation: `min_drop_ratio <= dropped_oldest/submitted <= max_drop_ratio`
4. lag pressure present: `lag_p95_ms >= min_lag_p95_ms`
5. degradation signal present: `live_chunk_queue_drop_oldest`
6. trust signal present: `chunk_queue_backpressure`
7. reconciliation signal present: `reconciliation_applied_after_backpressure`
8. JSONL includes `event_type=chunk_queue`

This keeps backlog pressure as a measured quality gate rather than an undefined edge case.
