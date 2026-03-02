#!/usr/bin/env bash
set -euo pipefail

tests=(
  contract_ci_enforcement
  recordit_cli_contract
  recordit_cli_dispatch
  recordit_exit_contract
  recordit_exit_behavior_matrix
  transcribe_live_legacy_entrypoints_compat
  runtime_mode_matrix_contract
  runtime_jsonl_contract
  runtime_jsonl_schema_contract
  runtime_manifest_contract
  runtime_manifest_schema_contract
  contract_baseline_matrix
  bd_1n5v_contract_regression
)

echo "[contracts-ci] running contract/schema enforcement suite"
for test_name in "${tests[@]}"; do
  echo "[contracts-ci] cargo test --test ${test_name} -- --nocapture"
  cargo test --test "${test_name}" -- --nocapture
done
echo "[contracts-ci] all contract/schema checks passed"
