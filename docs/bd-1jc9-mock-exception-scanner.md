# bd-1jc9 Mock/Fake Scanner With Exception Enforcement

This lane enforces coverage honesty for seam-bearing critical-path surfaces.

## Inputs

- `docs/bd-39i6-canonical-downstream-matrix.csv`
- `docs/bd-39i6-critical-surface-coverage-matrix.csv`
- `docs/bd-2mbp-critical-path-exception-register.csv`

## What It Checks

1. Seam-bearing canonical surfaces have tracked exception rows.
2. Tracked rows include required metadata (`owner_area`, `replacement_bead`).
3. Tracked rows use valid non-expired `expires_at_utc`.
4. Layer-level seam inventory is emitted for downstream CI/reporting.

## Commands

Strict policy mode (fails CI on violations):

```bash
scripts/gate_mock_exception_register.sh --policy-mode fail
```

Warn policy mode (required-fail warning output, zero exit):

```bash
scripts/gate_mock_exception_register.sh --policy-mode warn
```

## Output

- `summary.csv`: key/value counters (`missing_exception_count`,
  `expired_exception_count`, `missing_metadata_count`, layer inventory JSON).
- `status.json`: structured violations and metrics.
- `status.txt`: one-line gate verdict and artifact pointers.
