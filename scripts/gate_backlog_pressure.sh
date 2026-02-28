#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
MODEL="${MODEL:-$ROOT/artifacts/bench/models/whispercpp/ggml-tiny.en.bin}"
FIXTURE="${FIXTURE:-$ROOT/artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav}"
OUT_DIR="${OUT_DIR:-}"
DURATION_SEC="${DURATION_SEC:-3}"
CHUNK_WINDOW_MS="${CHUNK_WINDOW_MS:-1200}"
CHUNK_STRIDE_MS="${CHUNK_STRIDE_MS:-120}"
CHUNK_QUEUE_CAP="${CHUNK_QUEUE_CAP:-2}"
MIN_DROP_RATIO="${MIN_DROP_RATIO:-0.15}"
MAX_DROP_RATIO="${MAX_DROP_RATIO:-0.80}"
MIN_LAG_P95_MS="${MIN_LAG_P95_MS:-240}"

usage() {
  cat <<USAGE
Usage: $0 [options]

Runs a deterministic near-live backlog-pressure gate scenario by exercising
the live-chunked queue contract with a deterministic fake capture harness.

Options:
  --out-dir PATH            Output directory (default: artifacts/bench/gate_backlog_pressure/<utc-stamp>)
  --model PATH              ASR model path (default: artifacts/bench/models/whispercpp/ggml-tiny.en.bin)
  --fixture PATH            Deterministic stereo fixture for fake capture (default: artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav)
  --duration-sec N          Runtime duration passed to capture contract (default: 3)
  --chunk-window-ms N       Near-live chunk window for pressure scenario (default: 1200)
  --chunk-stride-ms N       Near-live chunk stride for pressure scenario (default: 120)
  --chunk-queue-cap N       Near-live queue capacity for pressure scenario (default: 2)
  --min-drop-ratio F        Minimum acceptable dropped_oldest/submitted ratio (default: 0.15)
  --max-drop-ratio F        Maximum acceptable dropped_oldest/submitted ratio (default: 0.80)
  --min-lag-p95-ms N        Minimum acceptable lag_p95_ms (default: 240)
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
    --min-drop-ratio)
      MIN_DROP_RATIO="$2"
      shift 2
      ;;
    --max-drop-ratio)
      MAX_DROP_RATIO="$2"
      shift 2
      ;;
    --min-lag-p95-ms)
      MIN_LAG_P95_MS="$2"
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
  OUT_DIR="$ROOT/artifacts/bench/gate_backlog_pressure/$STAMP"
fi

if [[ ! -f "$MODEL" ]]; then
  echo "error: model does not exist: $MODEL" >&2
  exit 2
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "error: fixture does not exist: $FIXTURE" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"
RUN_STDOUT="$OUT_DIR/runtime.stdout.log"
RUN_TIME="$OUT_DIR/runtime.time.txt"
RUN_JSONL="$OUT_DIR/runtime.jsonl"
RUN_MANIFEST="$OUT_DIR/runtime.manifest.json"
RUN_OUT_WAV="$OUT_DIR/runtime.session.wav"
RUN_INPUT_WAV="$OUT_DIR/runtime.capture.wav"
SUMMARY_CSV="$OUT_DIR/summary.csv"
STATUS_TXT="$OUT_DIR/status.txt"

(
  cd "$ROOT"
  DYLD_LIBRARY_PATH=/usr/lib/swift cargo build --quiet --bin transcribe-live
)

BIN="$ROOT/target/debug/transcribe-live"
if [[ ! -x "$BIN" ]]; then
  echo "error: expected executable not found: $BIN" >&2
  exit 1
fi

HARNESS_BIN_DIR="$OUT_DIR/harness/bin"
mkdir -p "$HARNESS_BIN_DIR"
cp "$BIN" "$HARNESS_BIN_DIR/transcribe-live"

cat >"$HARNESS_BIN_DIR/sequoia_capture" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 5 ]]; then
  echo "usage: sequoia_capture <duration_sec> <output_wav> <sample_rate_hz> <mismatch_policy> <callback_mode>" >&2
  exit 2
fi

duration_sec="$1"
output_wav="$2"
sample_rate_hz="$3"
mismatch_policy="$4"
callback_mode="$5"
fixture="${RECORDIT_FAKE_CAPTURE_FIXTURE:?missing RECORDIT_FAKE_CAPTURE_FIXTURE}"

mkdir -p "$(dirname "$output_wav")"
cp "$fixture" "$output_wav"

stem="$(basename "$output_wav")"
stem="${stem%.*}"
telemetry_path="$(dirname "$output_wav")/${stem}.telemetry.json"
cat >"$telemetry_path" <<JSON
{
  "output_wav_path": "$output_wav",
  "duration_secs": $duration_sec,
  "target_rate_hz": $sample_rate_hz,
  "output_rate_hz": $sample_rate_hz,
  "restart_count": 0,
  "sample_rate_policy": {
    "mismatch_policy": "$mismatch_policy",
    "target_rate_hz": $sample_rate_hz,
    "output_rate_hz": $sample_rate_hz,
    "mic_input_rate_hz": $sample_rate_hz,
    "system_input_rate_hz": $sample_rate_hz,
    "mic_resampled_chunks": 0,
    "system_resampled_chunks": 0
  },
  "callback_mode": "$callback_mode"
}
JSON
EOF
chmod +x "$HARNESS_BIN_DIR/sequoia_capture"

set +e
(
  cd "$ROOT"
  /usr/bin/time -l env DYLD_LIBRARY_PATH=/usr/lib/swift RECORDIT_FAKE_CAPTURE_FIXTURE="$FIXTURE" "$HARNESS_BIN_DIR/transcribe-live" \
    --duration-sec "$DURATION_SEC" \
    --live-chunked \
    --input-wav "$RUN_INPUT_WAV" \
    --out-wav "$RUN_OUT_WAV" \
    --out-jsonl "$RUN_JSONL" \
    --out-manifest "$RUN_MANIFEST" \
    --asr-backend whispercpp \
    --asr-model "$MODEL" \
    --benchmark-runs 1 \
    --transcribe-channels mixed-fallback \
    --chunk-window-ms "$CHUNK_WINDOW_MS" \
    --chunk-stride-ms "$CHUNK_STRIDE_MS" \
    --chunk-queue-cap "$CHUNK_QUEUE_CAP"
) >"$RUN_STDOUT" 2>"$RUN_TIME"
EXIT_CODE=$?
set -e

if [[ "$EXIT_CODE" -ne 0 ]]; then
  cat >"$STATUS_TXT" <<STATUS
status=failed
detail=runtime_exit_code_${EXIT_CODE}
stdout_path=$RUN_STDOUT
time_path=$RUN_TIME
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS
  echo "GATE_BACKLOG_PRESSURE_OUT=$OUT_DIR"
  exit "$EXIT_CODE"
fi

python3 "$ROOT/scripts/gate_backlog_pressure_summary.py" \
  --manifest "$RUN_MANIFEST" \
  --jsonl "$RUN_JSONL" \
  --summary-csv "$SUMMARY_CSV" \
  --min-drop-ratio "$MIN_DROP_RATIO" \
  --max-drop-ratio "$MAX_DROP_RATIO" \
  --min-lag-p95-ms "$MIN_LAG_P95_MS" >/dev/null

GATE_PASS="$(awk -F, '$1=="gate_pass"{print $2}' "$SUMMARY_CSV" | tail -n 1 | tr -d '\r')"
if [[ "$GATE_PASS" == "true" ]]; then
  status="pass"
  detail="backlog_pressure_thresholds_satisfied"
else
  status="failed"
  detail="backlog_pressure_thresholds_failed"
fi

cat >"$STATUS_TXT" <<STATUS
status=$status
detail=$detail
summary_path=$SUMMARY_CSV
manifest_path=$RUN_MANIFEST
jsonl_path=$RUN_JSONL
stdout_path=$RUN_STDOUT
time_path=$RUN_TIME
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
STATUS

echo "GATE_BACKLOG_PRESSURE_OUT=$OUT_DIR"
if [[ "$GATE_PASS" != "true" ]]; then
  exit 1
fi
