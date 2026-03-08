# bd-2grd — Shared Evidence Helper Adoption Note

Date: 2026-03-06
Related bead: `bd-2grd`

## What landed in this slice

Shared helper:

- `scripts/e2e_evidence_lib.sh`

Current adopters:

- `scripts/ci_recordit_xctest_evidence.sh`
- `scripts/verify_recordit_release_context.sh`

## Standardized outputs now available

### Common metadata file

Both adopters now emit:

- `metadata.json`

Fields:

- `scenario_id`
- `artifact_track`
- `generated_at_utc`
- `evidence_root`
- `logs_dir`
- `artifacts_dir`
- `summary_csv`
- `status_csv`
- `script_path`

### Machine-readable summary companions

`ci_recordit_xctest_evidence.sh` now emits:

- `summary.csv`
- `summary.json`
- `status.csv`
- `status.json`
- `responsiveness_budget_summary.csv`
- `responsiveness_budget_summary.json`

`verify_recordit_release_context.sh` now emits:

- `summary.csv`
- `summary.json`
- `checks.json`
- `paths.env`

## Why this matters

This is not the full evidence contract yet. It is the first implementation seam that makes downstream work easier:

- `bd-13tm` can validate JSON companions without parsing only ad-hoc CSV layouts
- `bd-3co8`, `bd-78qy`, and `bd-v502` can adopt the same helper instead of inventing new summary extraction logic
- `bd-3p9b` can reason about evidence presence using stable file names across lanes

## Intentional scope limit

This slice does **not** define the final policy/schema by itself. It deliberately stays below that layer:

- no validator logic here
- no schema file here
- no rewrite of all gate scripts here

Those remain compatible follow-on work for the other `bd-2grd` slices already underway.
