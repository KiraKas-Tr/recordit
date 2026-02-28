# Mixed-Rate Regression Scenario (bd-2nh)

## Goal

Provide a repeatable capture regression that proves worker-side sample-rate adaptation is still functioning and that the transport remains healthy under a real mixed-rate capture run.

## Command

```bash
scripts/mixed_rate_regression.sh
```

Optional overrides:

```bash
scripts/mixed_rate_regression.sh \
  --seconds 10 \
  --sample-rate-hz 48000 \
  --mismatch-policy adapt-stream-rate \
  --callback-mode warn \
  --out-dir artifacts/bench/mixed_rate/<stamp>
```

## Artifact Root

Default artifact root:

- `artifacts/bench/mixed_rate/<utc-stamp>/`

Artifacts written per run:

- `capture.wav`
- `capture.telemetry.json`
- `capture.stdout.log`
- `capture.stderr.log`
- `summary.csv`
- `status.txt`

## Pass Criteria

A run is considered a valid mixed-rate regression pass when `summary.csv` reports:

- `policy=adapt-stream-rate`
- `mismatch_observed=true`
- `adaptation_observed=true`
- `transport_healthy=true`
- `scenario_pass=true`

Interpretation:

- `mismatch_observed=true` means at least one input stream rate differed from the requested target rate.
- `adaptation_observed=true` means the telemetry recorded resampled chunks on at least one channel.
- `transport_healthy=true` means there were no restarts, queue-full drops, slot-miss drops, fill failures, recycle failures, or oversized callback chunks.

## Failure Interpretation

Common failure modes:

- `capture_exit_code_*` in `status.txt`
  - capture failed before telemetry validation; inspect `capture.stderr.log`
- `missing_telemetry`
  - recorder did not emit the expected telemetry artifact
- `mixed_rate_acceptance_failed`
  - scenario ran, but one or more acceptance fields in `summary.csv` did not meet the regression gate

If a run fails because `mismatch_observed=false`, the machine likely did not present an actual mixed-rate condition and the artifact should not be used as a regression baseline.
