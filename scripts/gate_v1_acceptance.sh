#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
source "$ROOT/scripts/e2e_evidence_lib.sh"
MODEL="${MODEL:-$ROOT/artifacts/bench/models/whispercpp/ggml-tiny.en.bin}"
FIXTURE="${FIXTURE:-$ROOT/artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav}"
OUT_DIR="${OUT_DIR:-}"
DURATION_SEC="${DURATION_SEC:-3}"
CHUNK_WINDOW_MS="${CHUNK_WINDOW_MS:-2000}"
CHUNK_STRIDE_MS="${CHUNK_STRIDE_MS:-500}"
CHUNK_QUEUE_CAP="${CHUNK_QUEUE_CAP:-4}"

usage() {
  cat <<USAGE
Usage: $0 [options]

Runs deterministic v1 acceptance checks for true live-stream runtime behavior:
- cold start first stable transcript emission during active runtime
- warm start first stable transcript emission during active runtime
- artifact truth checks for cold/warm runs
- non-blocking/degradation/trust checks via gate_backlog_pressure

Options:
  --out-dir PATH            Output directory (default: artifacts/bench/gate_v1_acceptance/<utc-stamp>)
  --model PATH              ASR model path (default: artifacts/bench/models/whispercpp/ggml-tiny.en.bin)
  --fixture PATH            Deterministic stereo fixture (default: artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav)
  --duration-sec N          Duration for cold/warm runs (default: 3)
  --chunk-window-ms N       Cold/warm live-stream chunk window (default: 2000)
  --chunk-stride-ms N       Cold/warm live-stream chunk stride (default: 500)
  --chunk-queue-cap N       Cold/warm live-stream queue cap (default: 4)
  -h, --help                Show this help text
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --model)
      MODEL="$2"
      shift 2
      ;;
    --fixture)
      FIXTURE="$2"
      shift 2
      ;;
    --duration-sec)
      DURATION_SEC="$2"
      shift 2
      ;;
    --chunk-window-ms)
      CHUNK_WINDOW_MS="$2"
      shift 2
      ;;
    --chunk-stride-ms)
      CHUNK_STRIDE_MS="$2"
      shift 2
      ;;
    --chunk-queue-cap)
      CHUNK_QUEUE_CAP="$2"
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
  OUT_DIR="$ROOT/artifacts/bench/gate_v1_acceptance/$STAMP"
fi

if [[ ! -f "$MODEL" ]]; then
  echo "error: model does not exist: $MODEL" >&2
  exit 2
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "error: fixture does not exist: $FIXTURE" >&2
  exit 2
fi

LOG_DIR="$OUT_DIR/logs"
SUMMARY_JSON="$OUT_DIR/summary.json"
STATUS_JSON="$OUT_DIR/status.json"
METADATA_JSON="$OUT_DIR/metadata.json"
BUILD_LOG="$LOG_DIR/build_transcribe_live.log"
BACKLOG_STDOUT_LOG="$LOG_DIR/backlog_pressure.stdout.log"
BACKLOG_STDERR_LOG="$LOG_DIR/backlog_pressure.stderr.log"

mkdir -p "$OUT_DIR/cold" "$OUT_DIR/warm" "$LOG_DIR"

(
  cd "$ROOT"
  {
    echo "[gate-v1-acceptance] generated_at_utc=$(evidence_timestamp)"
    echo "[gate-v1-acceptance] cmd=DYLD_LIBRARY_PATH=/usr/lib/swift cargo build --quiet --bin transcribe-live"
    DYLD_LIBRARY_PATH=/usr/lib/swift cargo build --quiet --bin transcribe-live
  } >"$BUILD_LOG" 2>&1
)

BIN="$ROOT/target/debug/transcribe-live"
if [[ ! -x "$BIN" ]]; then
  echo "error: expected executable not found: $BIN" >&2
  exit 1
fi

run_live_case() {
  local case_name="$1"
  local case_dir="$OUT_DIR/$case_name"
  local input_wav="$case_dir/input.wav"
  local out_wav="$case_dir/session.wav"
  local out_jsonl="$case_dir/runtime.jsonl"
  local out_manifest="$case_dir/runtime.manifest.json"
  local stdout_log="$LOG_DIR/${case_name}.runtime.stdout.log"
  local time_log="$LOG_DIR/${case_name}.runtime.time.txt"

  mkdir -p "$case_dir"

  set +e
  (
    cd "$ROOT"
    /usr/bin/time -l env DYLD_LIBRARY_PATH=/usr/lib/swift RECORDIT_FAKE_CAPTURE_FIXTURE="$FIXTURE" "$BIN" \
      --duration-sec "$DURATION_SEC" \
      --live-stream \
      --input-wav "$input_wav" \
      --out-wav "$out_wav" \
      --out-jsonl "$out_jsonl" \
      --out-manifest "$out_manifest" \
      --asr-backend whispercpp \
      --asr-model "$MODEL" \
      --benchmark-runs 1 \
      --transcribe-channels mixed-fallback \
      --chunk-window-ms "$CHUNK_WINDOW_MS" \
      --chunk-stride-ms "$CHUNK_STRIDE_MS" \
      --chunk-queue-cap "$CHUNK_QUEUE_CAP"
  ) >"$stdout_log" 2>"$time_log"
  local exit_code=$?
  set -e

  if [[ "$exit_code" -ne 0 ]]; then
    echo "error: $case_name live run failed with exit code $exit_code" >&2
    echo "see: $stdout_log and $time_log" >&2
    exit "$exit_code"
  fi
}

run_live_case cold
run_live_case warm

BACKLOG_DIR="$OUT_DIR/backlog_pressure"
set +e
"$ROOT/scripts/gate_backlog_pressure.sh" \
  --out-dir "$BACKLOG_DIR" \
  --model "$MODEL" \
  --fixture "$FIXTURE" >"$BACKLOG_STDOUT_LOG" 2>"$BACKLOG_STDERR_LOG"
BACKLOG_EXIT_CODE=$?
set -e
if [[ "$BACKLOG_EXIT_CODE" -ne 0 ]]; then
  echo "error: backlog pressure gate failed with exit code $BACKLOG_EXIT_CODE" >&2
  echo "see: $BACKLOG_STDOUT_LOG and $BACKLOG_STDERR_LOG" >&2
  exit "$BACKLOG_EXIT_CODE"
fi

SUMMARY_CSV="$OUT_DIR/summary.csv"
STATUS_TXT="$OUT_DIR/status.txt"
evidence_write_metadata_json "$METADATA_JSON" "gate_v1_acceptance" "gate_v1_acceptance" "$OUT_DIR" "$LOG_DIR" "$OUT_DIR" "$SUMMARY_CSV" "$STATUS_TXT" "$0" "$SUMMARY_JSON" "$STATUS_JSON"

python3 "$ROOT/scripts/gate_v1_acceptance_summary.py" \
  --cold-manifest "$OUT_DIR/cold/runtime.manifest.json" \
  --cold-jsonl "$OUT_DIR/cold/runtime.jsonl" \
  --warm-manifest "$OUT_DIR/warm/runtime.manifest.json" \
  --warm-jsonl "$OUT_DIR/warm/runtime.jsonl" \
  --backlog-manifest "$BACKLOG_DIR/runtime.manifest.json" \
  --backlog-summary "$BACKLOG_DIR/summary.csv" \
  --summary-csv "$SUMMARY_CSV" >/dev/null

GATE_PASS="$(awk -F, '$1=="gate_pass"{print $2}' "$SUMMARY_CSV" | tail -n 1 | tr -d '\r')"
if [[ "$GATE_PASS" == "true" ]]; then
  status="pass"
  detail="v1_acceptance_thresholds_satisfied"
else
  status="failed"
  detail="v1_acceptance_thresholds_failed"
fi

evidence_csv_kv_to_json "$SUMMARY_CSV" "$SUMMARY_JSON"

cat >"$STATUS_TXT" <<STATUS
status=$status
detail=$detail
summary_path=$SUMMARY_CSV
summary_json=$SUMMARY_JSON
status_json=$STATUS_JSON
metadata_json=$METADATA_JSON
logs_dir=$LOG_DIR
build_log=$BUILD_LOG
cold_dir=$OUT_DIR/cold
warm_dir=$OUT_DIR/warm
backlog_dir=$BACKLOG_DIR
backlog_stdout_log=$BACKLOG_STDOUT_LOG
backlog_stderr_log=$BACKLOG_STDERR_LOG
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS

evidence_kv_text_to_json "$STATUS_TXT" "$STATUS_JSON"

echo "GATE_V1_ACCEPTANCE_OUT=$OUT_DIR"
echo "GATE_V1_ACCEPTANCE_SUMMARY_JSON=$SUMMARY_JSON"
echo "GATE_V1_ACCEPTANCE_STATUS_JSON=$STATUS_JSON"
if [[ "$GATE_PASS" != "true" ]]; then
  exit 1
fi
