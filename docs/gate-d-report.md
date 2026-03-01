# Gate D Near-Live Soak Report (bd-23u)

Date: 2026-02-28  
Status: refreshed for deterministic near-live runtime soak

## Scope

- Refresh Gate D so soak evidence exercises the near-live runtime path (`--live-chunked`) instead of legacy offline-only invocation.
- Preserve long-run reliability checks (duration, failures, latency drift, memory drift).
- Add near-live truth checks for queue/lag visibility, continuity telemetry, and out-wav materialization.

## Procedure

Run the gate:

```bash
make gate-d-soak
```

Implementation:
- `scripts/gate_d_soak.sh`
- `scripts/gate_d_summary.py`

The soak harness is deterministic and host-independent:
- builds `target/debug/transcribe-live`
- runs `transcribe-live` with `RECORDIT_FAKE_CAPTURE_FIXTURE=<fixture>` so the shared live capture runtime uses deterministic fixture input and emits capture telemetry
- runs repeated `--live-chunked` invocations while collecting per-run manifest/JSONL/time/stdout artifacts

## Artifact Layout

Gate root:
- `artifacts/bench/gate_d/<stamp>/`

Per-run artifacts:
- `runs/run_<id>.capture.wav`
- `runs/run_<id>.session.telemetry.json` (primary; `capture.telemetry` accepted as fallback for compatibility)
- `runs/run_<id>.session.wav`
- `runs/run_<id>.manifest.json`
- `runs/run_<id>.jsonl`
- `runs/run_<id>.stdout.log`
- `runs/run_<id>.time.txt`

Aggregates:
- `runs.csv`
- `summary.csv`
- `status.txt`

## Thresholds

| Check | Threshold |
|---|---|
| Soak duration | `soak_seconds_actual >= soak_seconds_target` |
| Harness reliability | `failure_count = 0` |
| Runtime latency drift | `manifest_wall_ms_p95_p95 <= 1.25 * manifest_wall_ms_p95_p50` |
| Memory growth | `max_rss_kb_p95 <= 1.30 * max_rss_kb_p50` |
| Near-live mode truth | `threshold_near_live_mode_ok=true` (runtime mode is live-chunked/near-live and `live_chunked=true`) |
| Chunk queue visibility | `threshold_chunk_queue_visibility_ok=true` |
| Chunk drain health | `threshold_chunk_drain_ok=true` |
| Out-wav truth | `threshold_out_wav_truth_ok=true` |
| Continuity signal presence | `threshold_continuity_signal_ok=true` |
| Lag drift stability | `threshold_lag_drift_ok=true` |

`summary.csv` publishes these booleans and `gate_pass`.

## Validation Evidence

Short validation soak (after refresh):
- `artifacts/bench/gate_d/20260228T154530Z/summary.csv`
- key outcomes: `run_count=2`, `failure_count=0`, `threshold_near_live_mode_ok=true`, `threshold_chunk_queue_visibility_ok=true`, `threshold_continuity_signal_ok=true`, `gate_pass=true`

Intermediate failing validation during implementation (before mode-label fix):
- `artifacts/bench/gate_d/20260228T154421Z/summary.csv`
- `gate_pass=false` solely because runtime mode field reported `live-chunked` while the initial threshold expected `near-live`
