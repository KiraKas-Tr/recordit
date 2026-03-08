#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
source "$ROOT/scripts/e2e_evidence_lib.sh"

SCENARIO_ID="packaged_readiness_parity_matrix"
OUT_DIR="${OUT_DIR:-}"
RECORDIT_RUNTIME_BIN="${RECORDIT_RUNTIME_BIN:-$ROOT/dist/Recordit.app/Contents/Resources/runtime/bin/recordit}"
CONTRACT_PATH="${CONTRACT_PATH:-$ROOT/contracts/readiness-contract-ids.v1.json}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
SKIP_BUILD="${SKIP_BUILD:-0}"

usage() {
  cat <<'USAGE'
Usage: gate_packaged_readiness_parity.sh [options]

Run packaged-app readiness parity matrix with retained preflight payloads,
readiness-ID mapping, and action-gating decisions.

Required scenarios:
- missing-permission
- no-display
- runtime-preflight-failure
- fully-ready
- live-blocked-record-allowed-fallback

Options:
  --out-dir PATH                Output root (default: artifacts/validation/bd-2ysa/<utc-stamp>)
  --recordit-runtime-bin PATH   Embedded runtime binary (default: dist/Recordit.app/.../runtime/bin/recordit)
  --contract-path PATH          Readiness contract JSON (default: contracts/readiness-contract-ids.v1.json)
  --sign-identity VALUE         Codesign identity for build/sign step (default: -)
  --skip-build                  Skip make sign-recordit-app
  -h, --help                    Show this help text
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --recordit-runtime-bin)
      RECORDIT_RUNTIME_BIN="$2"
      shift 2
      ;;
    --contract-path)
      CONTRACT_PATH="$2"
      shift 2
      ;;
    --sign-identity)
      SIGN_IDENTITY="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required" >&2
  exit 2
fi

abs_path() {
  python3 - "$1" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).expanduser().resolve(strict=False))
PY
}

if [[ -z "$OUT_DIR" ]]; then
  STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
  OUT_DIR="$ROOT/artifacts/validation/bd-2ysa/gate_packaged_readiness_parity/$STAMP"
fi

OUT_DIR="$(abs_path "$OUT_DIR")"
RECORDIT_RUNTIME_BIN="$(abs_path "$RECORDIT_RUNTIME_BIN")"
CONTRACT_PATH="$(abs_path "$CONTRACT_PATH")"

LOG_DIR="$OUT_DIR/logs"
SCENARIOS_DIR="$OUT_DIR/scenarios"
ARTIFACTS_DIR="$OUT_DIR/artifacts"
PHASE_NOTES_DIR="$ARTIFACTS_DIR/phases"

SUMMARY_CSV="$ARTIFACTS_DIR/readiness_parity_matrix.csv"
SUMMARY_JSON="$ARTIFACTS_DIR/readiness_parity_matrix.json"
SUMMARY_STATUS_TXT="$ARTIFACTS_DIR/readiness_parity_matrix_status.txt"
SUMMARY_STATUS_JSON="$ARTIFACTS_DIR/readiness_parity_matrix_status.json"
PHASE_MANIFEST="$ARTIFACTS_DIR/phases.json"
METADATA_JSON="$OUT_DIR/metadata.json"
BUILD_LOG="$LOG_DIR/build_sign_recordit.log"

mkdir -p "$OUT_DIR" "$LOG_DIR" "$SCENARIOS_DIR" "$ARTIFACTS_DIR" "$PHASE_NOTES_DIR"

evidence_write_metadata_json \
  "$METADATA_JSON" \
  "$SCENARIO_ID" \
  "packaged-e2e" \
  "$OUT_DIR" \
  "$LOG_DIR" \
  "$ARTIFACTS_DIR" \
  "$SUMMARY_CSV" \
  "$SUMMARY_STATUS_TXT" \
  "$0" \
  "$SUMMARY_JSON" \
  "$SUMMARY_STATUS_JSON"

if [[ "$SKIP_BUILD" != "1" ]]; then
  set +e
  (
    cd "$ROOT"
    make sign-recordit-app SIGN_IDENTITY="$SIGN_IDENTITY"
  ) >"$BUILD_LOG" 2>&1
  BUILD_EXIT_CODE=$?
  set -e
  if [[ "$BUILD_EXIT_CODE" -ne 0 ]]; then
    cat > "$SUMMARY_STATUS_TXT" <<STATUS
status=fail
failure_stage=build_sign_recordit
build_exit_code=$BUILD_EXIT_CODE
build_log=$BUILD_LOG
STATUS
    evidence_kv_text_to_json "$SUMMARY_STATUS_TXT" "$SUMMARY_STATUS_JSON"
    exit 1
  fi
else
  printf 'skip-build requested\n' > "$BUILD_LOG"
fi

if [[ ! -x "$RECORDIT_RUNTIME_BIN" ]]; then
  echo "error: runtime binary missing or not executable: $RECORDIT_RUNTIME_BIN" >&2
  exit 2
fi
if [[ ! -f "$CONTRACT_PATH" ]]; then
  echo "error: readiness contract missing: $CONTRACT_PATH" >&2
  exit 2
fi

write_scenario_meta() {
  local meta_path="$1"
  local scenario_id="$2"
  local scenario_mode="$3"
  local description="$4"
  local expected_primary_blocking_domain="$5"
  local expected_blocking_ids="$6"
  local expected_can_proceed_without_ack="$7"
  local expected_can_proceed_with_ack="$8"
  local expected_record_only_fallback_eligible="$9"
  local expected_overall_status="${10}"
  local preflight_manifest_path="${11}"
  local session_root="${12}"
  local stdout_log="${13}"
  local stderr_log="${14}"

  python3 - "$meta_path" "$scenario_id" "$scenario_mode" "$description" "$expected_primary_blocking_domain" "$expected_blocking_ids" "$expected_can_proceed_without_ack" "$expected_can_proceed_with_ack" "$expected_record_only_fallback_eligible" "$expected_overall_status" "$preflight_manifest_path" "$session_root" "$stdout_log" "$stderr_log" <<'PY'
import json
import sys
from pathlib import Path

(
    meta_path,
    scenario_id,
    scenario_mode,
    description,
    expected_primary_blocking_domain,
    expected_blocking_ids,
    expected_can_proceed_without_ack,
    expected_can_proceed_with_ack,
    expected_record_only_fallback_eligible,
    expected_overall_status,
    preflight_manifest_path,
    session_root,
    stdout_log,
    stderr_log,
) = sys.argv[1:]

payload = {
    "scenario_id": scenario_id,
    "scenario_mode": scenario_mode,
    "description": description,
    "expected_primary_blocking_domain": expected_primary_blocking_domain,
    "expected_blocking_ids": [item for item in expected_blocking_ids.split("|") if item],
    "expected_can_proceed_without_ack": expected_can_proceed_without_ack.strip().lower() == "true",
    "expected_can_proceed_with_ack": expected_can_proceed_with_ack.strip().lower() == "true",
    "expected_record_only_fallback_eligible": expected_record_only_fallback_eligible.strip().lower() == "true",
    "expected_overall_status": expected_overall_status,
    "preflight_manifest_path": preflight_manifest_path,
    "session_root": session_root,
    "stdout_log": stdout_log,
    "stderr_log": stderr_log,
}

path = Path(meta_path)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

write_execution_json() {
  local execution_path="$1"
  local scenario_id="$2"
  local command_display="$3"
  local exit_code="$4"
  local session_root="$5"
  local stdout_log="$6"
  local stderr_log="$7"
  local started_at_utc="$8"
  local ended_at_utc="$9"
  local preflight_manifest_path="${10}"
  local working_directory="${11}"

  python3 - "$execution_path" "$scenario_id" "$command_display" "$exit_code" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest_path" "$working_directory" <<'PY'
import json
import sys
from pathlib import Path

(
    execution_path,
    scenario_id,
    command_display,
    exit_code,
    session_root,
    stdout_log,
    stderr_log,
    started_at_utc,
    ended_at_utc,
    preflight_manifest_path,
    working_directory,
) = sys.argv[1:]

payload = {
    "scenario_id": scenario_id,
    "command_display": command_display,
    "exit_code": int(exit_code) if exit_code not in {"", "none"} else None,
    "session_root": session_root,
    "stdout_log": stdout_log,
    "stderr_log": stderr_log,
    "started_at_utc": started_at_utc,
    "ended_at_utc": ended_at_utc,
    "preflight_manifest_path": preflight_manifest_path,
    "working_directory": working_directory,
}

path = Path(execution_path)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

write_preflight_fixture() {
  local output_path="$1"
  local overall_status="$2"
  local failing_ids="$3"
  local detail_hint="$4"

  python3 - "$output_path" "$overall_status" "$failing_ids" "$detail_hint" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

output_path, overall_status, failing_ids_raw, detail_hint = sys.argv[1:]
failing_ids = {item.strip() for item in failing_ids_raw.split(",") if item.strip()}

all_ids = [
    "model_path",
    "out_wav",
    "out_jsonl",
    "out_manifest",
    "sample_rate",
    "screen_capture_access",
    "display_availability",
    "microphone_access",
    "backend_runtime",
]

checks = []
for check_id in all_ids:
    status = "FAIL" if check_id in failing_ids else "PASS"
    checks.append(
        {
            "id": check_id,
            "status": status,
            "detail": f"synthetic {detail_hint}: {check_id} {status}",
            "remediation": "synthetic fixture" if status == "FAIL" else "",
        }
    )

payload = {
    "schema_version": "1",
    "kind": "transcribe-live-preflight",
    "generated_at_utc": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "overall_status": overall_status,
    "runtime_mode": "live-stream",
    "runtime_mode_taxonomy": "live-stream",
    "runtime_mode_selector": "--live-stream",
    "runtime_mode_status": "implemented",
    "config": {
        "input_wav": "/tmp/synthetic-session.input.wav",
        "out_wav": "/tmp/synthetic-session.wav",
        "out_jsonl": "/tmp/synthetic-session.jsonl",
        "out_manifest": "/tmp/synthetic-session.manifest.json",
        "asr_backend": "whispercpp",
        "asr_model_requested": "<synthetic>",
        "asr_model_resolved": "<synthetic>",
        "asr_model_source": "synthetic-fixture",
        "sample_rate_hz": 48000,
    },
    "checks": checks,
}

path = Path(output_path)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

run_missing_permission_fixture() {
  local scenario_id="missing-permission"
  local scenario_dir="$SCENARIOS_DIR/$scenario_id"
  local session_root="$scenario_dir/session"
  local meta_json="$scenario_dir/scenario_meta.json"
  local execution_json="$scenario_dir/execution.json"
  local stdout_log="$scenario_dir/stdout.log"
  local stderr_log="$scenario_dir/stderr.log"
  local preflight_manifest="$scenario_dir/preflight.manifest.json"

  mkdir -p "$scenario_dir" "$session_root"
  write_preflight_fixture "$preflight_manifest" "FAIL" "screen_capture_access,microphone_access" "missing-permission"

  printf '{"scenario":"%s","mode":"synthetic","input":"screen/microphone denied"}\n' "$scenario_id" >"$stdout_log"
  printf 'synthetic fixture: missing permission blockers\n' >"$stderr_log"

  local started_at_utc ended_at_utc
  started_at_utc="$(evidence_timestamp)"
  ended_at_utc="$(evidence_timestamp)"

  write_scenario_meta "$meta_json" "$scenario_id" "synthetic" "Synthetic missing-permission readiness payload" "tcc_capture" "screen_capture_access|microphone_access" "false" "false" "false" "FAIL" "$preflight_manifest" "$session_root" "$stdout_log" "$stderr_log"
  write_execution_json "$execution_json" "$scenario_id" "synthetic-preflight-fixture missing-permission" "2" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest" "$scenario_dir"
}

run_no_display_fixture() {
  local scenario_id="no-display"
  local scenario_dir="$SCENARIOS_DIR/$scenario_id"
  local session_root="$scenario_dir/session"
  local meta_json="$scenario_dir/scenario_meta.json"
  local execution_json="$scenario_dir/execution.json"
  local stdout_log="$scenario_dir/stdout.log"
  local stderr_log="$scenario_dir/stderr.log"
  local preflight_manifest="$scenario_dir/preflight.manifest.json"

  mkdir -p "$scenario_dir" "$session_root"
  write_preflight_fixture "$preflight_manifest" "FAIL" "display_availability" "no-display"

  printf '{"scenario":"%s","mode":"synthetic","input":"display unavailable"}\n' "$scenario_id" >"$stdout_log"
  printf 'synthetic fixture: no-display blocker\n' >"$stderr_log"

  local started_at_utc ended_at_utc
  started_at_utc="$(evidence_timestamp)"
  ended_at_utc="$(evidence_timestamp)"

  write_scenario_meta "$meta_json" "$scenario_id" "synthetic" "Synthetic no-display readiness payload" "tcc_capture" "display_availability" "false" "false" "false" "FAIL" "$preflight_manifest" "$session_root" "$stdout_log" "$stderr_log"
  write_execution_json "$execution_json" "$scenario_id" "synthetic-preflight-fixture no-display" "2" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest" "$scenario_dir"
}

run_runtime_preflight_failure_real() {
  local scenario_id="runtime-preflight-failure"
  local scenario_dir="$SCENARIOS_DIR/$scenario_id"
  local session_root="$scenario_dir/session"
  local meta_json="$scenario_dir/scenario_meta.json"
  local execution_json="$scenario_dir/execution.json"
  local stdout_log="$scenario_dir/stdout.log"
  local stderr_log="$scenario_dir/stderr.log"
  local preflight_manifest="$session_root/session.manifest.json"

  mkdir -p "$scenario_dir" "$session_root"
  mkdir -p "$session_root/session.wav"

  local started_at_utc ended_at_utc exit_code
  started_at_utc="$(evidence_timestamp)"
  set +e
  (
    cd "$ROOT"
    "$RECORDIT_RUNTIME_BIN" preflight --mode live --output-root "$session_root" --json >"$stdout_log" 2>"$stderr_log"
  )
  exit_code=$?
  set -e
  ended_at_utc="$(evidence_timestamp)"

  write_scenario_meta "$meta_json" "$scenario_id" "real" "Real packaged preflight with output-path runtime-preflight blocker" "runtime_preflight" "out_wav" "false" "false" "false" "FAIL" "$preflight_manifest" "$session_root" "$stdout_log" "$stderr_log"
  write_execution_json "$execution_json" "$scenario_id" "$RECORDIT_RUNTIME_BIN preflight --mode live --output-root $session_root --json" "$exit_code" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest" "$ROOT"
}

run_fully_ready_real() {
  local scenario_id="fully-ready"
  local scenario_dir="$SCENARIOS_DIR/$scenario_id"
  local session_root="$scenario_dir/session"
  local meta_json="$scenario_dir/scenario_meta.json"
  local execution_json="$scenario_dir/execution.json"
  local stdout_log="$scenario_dir/stdout.log"
  local stderr_log="$scenario_dir/stderr.log"
  local preflight_manifest="$session_root/session.manifest.json"

  mkdir -p "$scenario_dir" "$session_root"

  local started_at_utc ended_at_utc exit_code
  started_at_utc="$(evidence_timestamp)"
  set +e
  (
    cd "$ROOT"
    "$RECORDIT_RUNTIME_BIN" preflight --mode live --output-root "$session_root" --json >"$stdout_log" 2>"$stderr_log"
  )
  exit_code=$?
  set -e
  ended_at_utc="$(evidence_timestamp)"

  write_scenario_meta "$meta_json" "$scenario_id" "real" "Real packaged preflight fully-ready baseline" "none" "" "true" "true" "false" "PASS" "$preflight_manifest" "$session_root" "$stdout_log" "$stderr_log"
  write_execution_json "$execution_json" "$scenario_id" "$RECORDIT_RUNTIME_BIN preflight --mode live --output-root $session_root --json" "$exit_code" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest" "$ROOT"
}

run_live_blocked_record_only_fallback_real() {
  local scenario_id="live-blocked-record-allowed-fallback"
  local scenario_dir="$SCENARIOS_DIR/$scenario_id"
  local session_root="$scenario_dir/session"
  local workdir="$scenario_dir/workdir"
  local home_dir="$scenario_dir/home"
  local meta_json="$scenario_dir/scenario_meta.json"
  local execution_json="$scenario_dir/execution.json"
  local stdout_log="$scenario_dir/stdout.log"
  local stderr_log="$scenario_dir/stderr.log"
  local preflight_manifest="$session_root/session.manifest.json"

  mkdir -p "$scenario_dir" "$session_root" "$workdir" "$home_dir"

  local started_at_utc ended_at_utc exit_code
  started_at_utc="$(evidence_timestamp)"
  set +e
  (
    cd "$workdir"
    env HOME="$home_dir" "$RECORDIT_RUNTIME_BIN" preflight --mode live --output-root "$session_root" --json >"$stdout_log" 2>"$stderr_log"
  )
  exit_code=$?
  set -e
  ended_at_utc="$(evidence_timestamp)"

  write_scenario_meta "$meta_json" "$scenario_id" "real" "Real packaged preflight with backend-model blocker and Record Only fallback eligibility" "backend_model" "model_path" "false" "false" "true" "FAIL" "$preflight_manifest" "$session_root" "$stdout_log" "$stderr_log"
  write_execution_json "$execution_json" "$scenario_id" "(cd $workdir && HOME=$home_dir $RECORDIT_RUNTIME_BIN preflight --mode live --output-root $session_root --json)" "$exit_code" "$session_root" "$stdout_log" "$stderr_log" "$started_at_utc" "$ended_at_utc" "$preflight_manifest" "$workdir"
}

run_missing_permission_fixture
run_no_display_fixture
run_runtime_preflight_failure_real
run_fully_ready_real
run_live_blocked_record_only_fallback_real

set +e
python3 "$ROOT/scripts/gate_packaged_readiness_parity_summary.py" \
  --scenarios-root "$SCENARIOS_DIR" \
  --contract-path "$CONTRACT_PATH" \
  --summary-csv "$SUMMARY_CSV" \
  --summary-json "$SUMMARY_JSON" \
  --status-path "$SUMMARY_STATUS_TXT"
MATRIX_EXIT=$?
set -e

if [[ ! -f "$SUMMARY_STATUS_TXT" ]]; then
  cat >"$SUMMARY_STATUS_TXT" <<STATUS
status=fail
failure_stage=readiness_parity_summary
summary_exit_code=$MATRIX_EXIT
summary_csv=$SUMMARY_CSV
summary_json=$SUMMARY_JSON
STATUS
fi

evidence_kv_text_to_json "$SUMMARY_STATUS_TXT" "$SUMMARY_STATUS_JSON"

python3 - "$SUMMARY_CSV" "$OUT_DIR" "$PHASE_MANIFEST" "$PHASE_NOTES_DIR" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_csv = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
phase_manifest = Path(sys.argv[3])
phase_notes_dir = Path(sys.argv[4])

rows = []
with summary_csv.open(newline="", encoding="utf-8") as handle:
    rows = list(csv.DictReader(handle))

def now_utc() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")

phases: list[dict[str, object]] = []
phase_notes_dir.mkdir(parents=True, exist_ok=True)
for row in rows:
    scenario_id = row.get("scenario_id", "")
    started = row.get("started_at_utc") or now_utc()
    ended = row.get("ended_at_utc") or started

    stdout_log = Path(row.get("stdout_log") or "")
    stderr_log = Path(row.get("stderr_log") or "")
    if not stdout_log.is_absolute():
        stdout_log = (out_dir / stdout_log).resolve(strict=False)
    if not stderr_log.is_absolute():
        stderr_log = (out_dir / stderr_log).resolve(strict=False)

    note_path = phase_notes_dir / f"{scenario_id}.txt"
    note_payload = [
        f"scenario_id={scenario_id}",
        f"status={row.get('status', '')}",
        f"overall_status={row.get('overall_status', '')}",
        f"primary_blocking_domain={row.get('primary_blocking_domain', '')}",
        f"blocking_ids={row.get('blocking_ids', '')}",
        f"can_proceed_without_ack={row.get('can_proceed_without_ack', '')}",
        f"can_proceed_with_ack={row.get('can_proceed_with_ack', '')}",
        f"record_only_fallback_eligible={row.get('record_only_fallback_eligible', '')}",
    ]
    note_path.write_text("\n".join(note_payload) + "\n", encoding="utf-8")

    try:
        stdout_rel = str(stdout_log.relative_to(out_dir))
    except ValueError:
        stdout_rel = str(Path("logs") / f"{scenario_id}.stdout.log")
    try:
        stderr_rel = str(stderr_log.relative_to(out_dir))
    except ValueError:
        stderr_rel = str(Path("logs") / f"{scenario_id}.stderr.log")
    note_rel = str(note_path.relative_to(out_dir))

    phases.append(
        {
            "phase_id": scenario_id,
            "title": f"Readiness scenario: {scenario_id}",
            "required": True,
            "status": row.get("status", "fail"),
            "exit_classification": "success" if row.get("status") == "pass" else "product_failure",
            "started_at_utc": started,
            "ended_at_utc": ended,
            "command_display": f"scenario:{scenario_id}",
            "command_argv": ["scenario", scenario_id],
            "log_relpath": stdout_rel,
            "stdout_relpath": stdout_rel,
            "stderr_relpath": stderr_rel,
            "primary_artifact_relpath": note_rel,
            "notes": (
                f"expected_domain={row.get('expected_primary_blocking_domain','')} "
                f"observed_domain={row.get('primary_blocking_domain','')}"
            ),
        }
    )

phase_manifest.parent.mkdir(parents=True, exist_ok=True)
phase_manifest.write_text(json.dumps({"phases": phases}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

evidence_render_contract \
  "$OUT_DIR" \
  "$SCENARIO_ID" \
  "packaged-e2e" \
  "$PHASE_MANIFEST" \
  --generated-at-utc "$(evidence_timestamp)" \
  --artifact-root-relpath "artifacts" \
  --paths-env-entry "RECORDIT_RUNTIME_BIN=$RECORDIT_RUNTIME_BIN" \
  --paths-env-entry "CONTRACT_PATH=$CONTRACT_PATH" \
  --paths-env-entry "READINESS_MATRIX_CSV=$SUMMARY_CSV" \
  --paths-env-entry "READINESS_MATRIX_JSON=$SUMMARY_JSON" \
  --paths-env-entry "READINESS_MATRIX_STATUS_TXT=$SUMMARY_STATUS_TXT" \
  --paths-env-entry "READINESS_MATRIX_STATUS_JSON=$SUMMARY_STATUS_JSON"

MATRIX_STATUS="$(awk -F= '$1=="status" {print $2}' "$SUMMARY_STATUS_TXT" | tail -n 1)"
CONTRACT_STATUS="$(awk -F= '$1=="status" {print $2}' "$OUT_DIR/status.txt" | tail -n 1)"

cat >"$SUMMARY_STATUS_TXT" <<STATUS
status=$MATRIX_STATUS
contract_status=$CONTRACT_STATUS
readiness_parity_matrix_csv=$SUMMARY_CSV
readiness_parity_matrix_json=$SUMMARY_JSON
readiness_parity_matrix_status_json=$SUMMARY_STATUS_JSON
evidence_contract_root=$OUT_DIR
evidence_contract_summary=$OUT_DIR/summary.csv
evidence_contract_status=$OUT_DIR/status.txt
metadata_json=$METADATA_JSON
STATUS

evidence_kv_text_to_json "$SUMMARY_STATUS_TXT" "$SUMMARY_STATUS_JSON"

echo "GATE_PACKAGED_READINESS_PARITY_OUT=$OUT_DIR"
echo "GATE_PACKAGED_READINESS_PARITY_CSV=$SUMMARY_CSV"
echo "GATE_PACKAGED_READINESS_PARITY_JSON=$SUMMARY_JSON"
echo "GATE_PACKAGED_READINESS_PARITY_STATUS=$SUMMARY_STATUS_TXT"
echo "GATE_PACKAGED_READINESS_PARITY_STATUS_JSON=$SUMMARY_STATUS_JSON"

if [[ "$MATRIX_EXIT" -ne 0 || "$MATRIX_STATUS" != "pass" || "$CONTRACT_STATUS" == "fail" ]]; then
  exit 1
fi
