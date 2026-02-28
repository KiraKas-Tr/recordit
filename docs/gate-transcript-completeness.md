# Gate: Transcript Completeness Under Backlog Pressure

This gate validates that reconciliation materially improves transcript completeness
when near-live backlog pressure is intentionally induced.

It reuses the deterministic backlog-pressure scenario (`gate_backlog_pressure.sh`),
then compares replay readability:

- **pre-reconciliation**: `reconciled_final` events removed
- **post-reconciliation**: full runtime JSONL

## Run

```bash
scripts/gate_transcript_completeness.sh
```

Optional tuning:

```bash
scripts/gate_transcript_completeness.sh \
  --chunk-window-ms 1200 \
  --chunk-stride-ms 120 \
  --chunk-queue-cap 2 \
  --min-completeness-gain 0.25 \
  --min-post-completeness 0.95 \
  --max-pre-completeness 0.80
```

Artifacts are written to:

- `artifacts/bench/gate_transcript_completeness/<timestamp>/backlog_pressure/`
- `artifacts/bench/gate_transcript_completeness/<timestamp>/pre_replay.txt`
- `artifacts/bench/gate_transcript_completeness/<timestamp>/post_replay.txt`
- `artifacts/bench/gate_transcript_completeness/<timestamp>/summary.csv`
- `artifacts/bench/gate_transcript_completeness/<timestamp>/status.txt`

## Acceptance Bar

The gate passes only if all thresholds in `summary.csv` are true:

1. reconciliation artifacts exist (`reconciled_final` present)
2. backlog pressure is confirmed (`chunk_queue.dropped_oldest > 0`)
3. reconciliation signaling is explicit (`reconciliation_applied` trust + `reconciliation_applied_after_backpressure` degradation)
4. completeness gain is meaningful:
   - `post_completeness - pre_completeness >= min_completeness_gain`
   - `post_completeness >= min_post_completeness`
   - `pre_completeness <= max_pre_completeness`

Completeness is measured as per-channel canonical token coverage, using
`reconciled_final` transcript tokens as the channel reference set.
