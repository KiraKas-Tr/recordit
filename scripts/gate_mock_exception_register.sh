#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
OUT_DIR="${OUT_DIR:-}"
POLICY_MODE="${POLICY_MODE:-fail}"
CANONICAL_MATRIX_CSV="${CANONICAL_MATRIX_CSV:-$ROOT/docs/bd-39i6-canonical-downstream-matrix.csv}"
CRITICAL_SURFACE_CSV="${CRITICAL_SURFACE_CSV:-$ROOT/docs/bd-39i6-critical-surface-coverage-matrix.csv}"
EXCEPTION_REGISTER_CSV="${EXCEPTION_REGISTER_CSV:-$ROOT/docs/bd-2mbp-critical-path-exception-register.csv}"

usage() {
  cat <<USAGE
Usage: $0 [options]

Scans critical-path seam usage and enforces exception-register coverage/expiry.

Options:
  --out-dir PATH                Output directory (default: artifacts/ci/gate_mock_exception_register/<utc-stamp>)
  --policy-mode MODE            fail|warn (default: fail)
  --canonical-matrix-csv PATH   Canonical downstream matrix CSV
  --critical-surface-csv PATH   Critical surface matrix CSV
  --exception-register-csv PATH Exception register CSV
  -h, --help                    Show this help text
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --policy-mode)
      POLICY_MODE="$2"
      shift 2
      ;;
    --canonical-matrix-csv)
      CANONICAL_MATRIX_CSV="$2"
      shift 2
      ;;
    --critical-surface-csv)
      CRITICAL_SURFACE_CSV="$2"
      shift 2
      ;;
    --exception-register-csv)
      EXCEPTION_REGISTER_CSV="$2"
      shift 2
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

if [[ -z "$OUT_DIR" ]]; then
  STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
  OUT_DIR="$ROOT/artifacts/ci/gate_mock_exception_register/$STAMP"
fi

mkdir -p "$OUT_DIR"
SUMMARY_CSV="$OUT_DIR/summary.csv"
STATUS_JSON="$OUT_DIR/status.json"
STATUS_TXT="$OUT_DIR/status.txt"

set +e
python3 "$ROOT/scripts/gate_mock_exception_register.py" \
  --policy-mode "$POLICY_MODE" \
  --canonical-matrix-csv "$CANONICAL_MATRIX_CSV" \
  --critical-surface-csv "$CRITICAL_SURFACE_CSV" \
  --exception-register-csv "$EXCEPTION_REGISTER_CSV" \
  --summary-csv "$SUMMARY_CSV" \
  --status-json "$STATUS_JSON"
EXIT_CODE=$?
set -e

if [[ "$EXIT_CODE" -eq 0 ]]; then
  status="pass"
  detail="policy_mode_${POLICY_MODE}_accepted"
else
  status="failed"
  detail="policy_mode_${POLICY_MODE}_violations_detected"
fi

cat >"$STATUS_TXT" <<STATUS
status=$status
detail=$detail
policy_mode=$POLICY_MODE
summary_path=$SUMMARY_CSV
status_json_path=$STATUS_JSON
canonical_matrix_csv_path=$CANONICAL_MATRIX_CSV
critical_surface_csv_path=$CRITICAL_SURFACE_CSV
exception_register_csv_path=$EXCEPTION_REGISTER_CSV
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS

echo "GATE_MOCK_EXCEPTION_REGISTER_OUT=$OUT_DIR"
exit "$EXIT_CODE"
