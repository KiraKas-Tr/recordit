#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
SECONDS="${SECONDS:-10}"
SAMPLE_RATE_HZ="${SAMPLE_RATE_HZ:-48000}"
MISMATCH_POLICY="${MISMATCH_POLICY:-adapt-stream-rate}"
CALLBACK_MODE="${CALLBACK_MODE:-warn}"
OUT_DIR="${OUT_DIR:-}"

usage() {
  cat <<USAGE
Usage: $0 [--seconds N] [--sample-rate-hz HZ] [--mismatch-policy POLICY] [--callback-mode MODE] [--out-dir PATH]

Runs a mixed-rate capture regression scenario and writes machine-readable evidence artifacts.
Defaults:
  --seconds 10
  --sample-rate-hz 48000
  --mismatch-policy adapt-stream-rate
  --callback-mode warn
  --out-dir artifacts/bench/mixed_rate/<utc-stamp>
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seconds)
      SECONDS="$2"
      shift 2
      ;;
    --sample-rate-hz)
      SAMPLE_RATE_HZ="$2"
      shift 2
      ;;
    --mismatch-policy)
      MISMATCH_POLICY="$2"
      shift 2
      ;;
    --callback-mode)
      CALLBACK_MODE="$2"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="$2"
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

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for mixed-rate regression summaries" >&2
  exit 2
fi

if [[ -z "$OUT_DIR" ]]; then
  STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
  OUT_DIR="$ROOT/artifacts/bench/mixed_rate/$STAMP"
fi

mkdir -p "$OUT_DIR"
OUT_WAV="$OUT_DIR/capture.wav"
OUT_STDOUT="$OUT_DIR/capture.stdout.log"
OUT_STDERR="$OUT_DIR/capture.stderr.log"
OUT_SUMMARY="$OUT_DIR/summary.csv"
OUT_STATUS="$OUT_DIR/status.txt"
OUT_TELEMETRY="$OUT_DIR/capture.telemetry.json"

(
  cd "$ROOT"
  DYLD_LIBRARY_PATH=/usr/lib/swift cargo build --quiet --bin sequoia_capture
)
BIN="$ROOT/target/debug/sequoia_capture"
if [[ ! -x "$BIN" ]]; then
  echo "error: expected executable not found: $BIN" >&2
  exit 1
fi

set +e
(
  cd "$ROOT"
  DYLD_LIBRARY_PATH=/usr/lib/swift "$BIN" "$SECONDS" "$OUT_WAV" "$SAMPLE_RATE_HZ" "$MISMATCH_POLICY" "$CALLBACK_MODE"
) >"$OUT_STDOUT" 2>"$OUT_STDERR"
EXIT_CODE=$?
set -e

if [[ "$EXIT_CODE" -ne 0 ]]; then
  cat >"$OUT_STATUS" <<STATUS
status=failed
detail=capture_exit_code_${EXIT_CODE}
telemetry_path=$OUT_TELEMETRY
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS
  echo "MIXED_RATE_OUT=$OUT_DIR"
  exit "$EXIT_CODE"
fi

if [[ ! -f "$OUT_TELEMETRY" ]]; then
  cat >"$OUT_STATUS" <<STATUS
status=failed
detail=missing_telemetry
telemetry_path=$OUT_TELEMETRY
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS
  echo "MIXED_RATE_OUT=$OUT_DIR"
  exit 1
fi

mismatch_policy="$(jq -r '.sample_rate_policy.mismatch_policy // "unknown"' "$OUT_TELEMETRY")"
target_rate_hz="$(jq -r '.sample_rate_policy.target_rate_hz // 0' "$OUT_TELEMETRY")"
output_rate_hz="$(jq -r '.sample_rate_policy.output_rate_hz // 0' "$OUT_TELEMETRY")"
mic_input_rate_hz="$(jq -r '.sample_rate_policy.mic_input_rate_hz // 0' "$OUT_TELEMETRY")"
system_input_rate_hz="$(jq -r '.sample_rate_policy.system_input_rate_hz // 0' "$OUT_TELEMETRY")"
mic_resampled_chunks="$(jq -r '.sample_rate_policy.mic_resampled_chunks // 0' "$OUT_TELEMETRY")"
system_resampled_chunks="$(jq -r '.sample_rate_policy.system_resampled_chunks // 0' "$OUT_TELEMETRY")"
restart_count="$(jq -r '.restart_count // 0' "$OUT_TELEMETRY")"
slot_miss_drops="$(jq -r '.transport.slot_miss_drops // 0' "$OUT_TELEMETRY")"
fill_failures="$(jq -r '.transport.fill_failures // 0' "$OUT_TELEMETRY")"
queue_full_drops="$(jq -r '.transport.queue_full_drops // 0' "$OUT_TELEMETRY")"
recycle_failures="$(jq -r '.transport.recycle_failures // 0' "$OUT_TELEMETRY")"
chunk_too_large="$(jq -r '.callback_audit.chunk_too_large // 0' "$OUT_TELEMETRY")"

mismatch_observed=false
if [[ "$mic_input_rate_hz" != "$target_rate_hz" || "$system_input_rate_hz" != "$target_rate_hz" ]]; then
  mismatch_observed=true
fi

adaptation_observed=false
if [[ "$mic_resampled_chunks" -gt 0 || "$system_resampled_chunks" -gt 0 ]]; then
  adaptation_observed=true
fi

transport_healthy=false
if [[ "$restart_count" -eq 0 && "$slot_miss_drops" -eq 0 && "$fill_failures" -eq 0 && "$queue_full_drops" -eq 0 && "$recycle_failures" -eq 0 && "$chunk_too_large" -eq 0 ]]; then
  transport_healthy=true
fi

scenario_pass=false
if [[ "$mismatch_policy" == "adapt-stream-rate" && "$mismatch_observed" == "true" && "$adaptation_observed" == "true" && "$transport_healthy" == "true" ]]; then
  scenario_pass=true
fi

cat >"$OUT_SUMMARY" <<CSV
artifact_track,policy,target_rate_hz,output_rate_hz,mic_input_rate_hz,system_input_rate_hz,mic_resampled_chunks,system_resampled_chunks,mismatch_observed,adaptation_observed,restart_count,slot_miss_drops,fill_failures,queue_full_drops,recycle_failures,chunk_too_large,transport_healthy,scenario_pass
mixed_rate_regression,$mismatch_policy,$target_rate_hz,$output_rate_hz,$mic_input_rate_hz,$system_input_rate_hz,$mic_resampled_chunks,$system_resampled_chunks,$mismatch_observed,$adaptation_observed,$restart_count,$slot_miss_drops,$fill_failures,$queue_full_drops,$recycle_failures,$chunk_too_large,$transport_healthy,$scenario_pass
CSV

if [[ "$scenario_pass" == "true" ]]; then
  status="pass"
  detail="mixed_rate_adaptation_confirmed"
else
  status="failed"
  detail="mixed_rate_acceptance_failed"
fi

cat >"$OUT_STATUS" <<STATUS
status=$status
detail=$detail
telemetry_path=$OUT_TELEMETRY
summary_path=$OUT_SUMMARY
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS

echo "MIXED_RATE_OUT=$OUT_DIR"
if [[ "$scenario_pass" != "true" ]]; then
  exit 1
fi
