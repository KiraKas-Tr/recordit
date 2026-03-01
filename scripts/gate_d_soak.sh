#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
SOAK_SECONDS="${SOAK_SECONDS:-3600}"
MODEL="${MODEL:-$ROOT/artifacts/bench/models/whispercpp/ggml-tiny.en.bin}"
FIXTURE="${FIXTURE:-$ROOT/artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav}"
OUT_DIR="${OUT_DIR:-}"
RUN_DURATION_SEC="${RUN_DURATION_SEC:-3}"
CHUNK_WINDOW_MS="${CHUNK_WINDOW_MS:-4000}"
CHUNK_STRIDE_MS="${CHUNK_STRIDE_MS:-1000}"
CHUNK_QUEUE_CAP="${CHUNK_QUEUE_CAP:-4}"
LLM_ENDPOINT="${LLM_ENDPOINT:-http://127.0.0.1:9/v1/chat/completions}"
LLM_MODEL="${LLM_MODEL:-dummy}"
LLM_TIMEOUT_MS="${LLM_TIMEOUT_MS:-80}"
LLM_MAX_QUEUE="${LLM_MAX_QUEUE:-1}"
LLM_RETRIES="${LLM_RETRIES:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seconds)
      SOAK_SECONDS="$2"
      shift 2
      ;;
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
      RUN_DURATION_SEC="$2"
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
    *)
      echo "unknown argument: $1" >&2
      echo "usage: $0 [--seconds N] [--out-dir PATH] [--model PATH] [--fixture PATH] [--duration-sec N] [--chunk-window-ms N] [--chunk-stride-ms N] [--chunk-queue-cap N]" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$OUT_DIR" ]]; then
  STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
  OUT_DIR="$ROOT/artifacts/bench/gate_d/$STAMP"
fi

mkdir -p "$OUT_DIR/runs"
if [[ ! -f "$MODEL" ]]; then
  echo "missing model: $MODEL" >&2
  exit 1
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "missing fixture: $FIXTURE" >&2
  exit 1
fi

echo "run_index,start_utc,end_utc,exit_code,real_ms,max_rss_kb,runtime_mode,live_chunked,mode_requested,mode_active,out_wav_materialized,out_wav_bytes,chunk_queue_enabled,chunk_max_queue,chunk_submitted,chunk_enqueued,chunk_dropped_oldest,chunk_processed,chunk_pending,chunk_high_water,chunk_drain_completed,chunk_lag_sample_count,chunk_lag_p50_ms,chunk_lag_p95_ms,chunk_lag_max_ms,trust_notice_count,degradation_event_count,reconciliation_applied,capture_restart_count,capture_telemetry_readable,manifest_wall_ms_p95,manifest_wall_ms_p50,cleanup_dropped_queue_full,cleanup_failed,cleanup_timed_out" >"$OUT_DIR/runs.csv"

(
  cd "$ROOT"
  DYLD_LIBRARY_PATH=/usr/lib/swift cargo build --quiet --bin transcribe-live
)
BIN="$ROOT/target/debug/transcribe-live"
if [[ ! -x "$BIN" ]]; then
  echo "missing executable: $BIN" >&2
  exit 1
fi

start_epoch="$(date +%s)"
end_epoch=$((start_epoch + SOAK_SECONDS))
run=0
termination_reason=""

write_status_marker() {
  local status="$1"
  local detail="$2"
  cat >"$OUT_DIR/status.txt" <<EOF
status=$status
detail=$detail
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF
}

emit_summary_if_possible() {
  if [[ ! -f "$OUT_DIR/runs.csv" ]]; then
    return 0
  fi
  if [[ "$(wc -l <"$OUT_DIR/runs.csv")" -le 1 ]]; then
    return 0
  fi
  python3 "$ROOT/scripts/gate_d_summary.py" \
    --runs-csv "$OUT_DIR/runs.csv" \
    --summary-csv "$OUT_DIR/summary.csv" \
    --target-seconds "$SOAK_SECONDS" >/dev/null 2>&1 || true
}

finalize_soak() {
  local exit_status="$1"
  trap - EXIT INT TERM HUP
  set +e

  if [[ -n "$termination_reason" ]]; then
    write_status_marker "interrupted" "$termination_reason"
  elif [[ "$exit_status" -eq 0 ]]; then
    write_status_marker "completed" "normal_exit"
  else
    write_status_marker "failed" "exit_status=$exit_status"
  fi

  emit_summary_if_possible
  return "$exit_status"
}

trap 'termination_reason="SIGINT"; exit 130' INT
trap 'termination_reason="SIGTERM"; exit 143' TERM
trap 'termination_reason="SIGHUP"; exit 129' HUP
trap 'finalize_soak $?' EXIT

while [[ "$(date +%s)" -lt "$end_epoch" ]]; do
  run=$((run + 1))
  run_id="$(printf "%05d" "$run")"
  base="$OUT_DIR/runs/run_$run_id"
  manifest="$base.manifest.json"
  jsonl="$base.jsonl"
  input_wav="$base.capture.wav"
  out_wav="$base.session.wav"
  telemetry_primary="$base.session.telemetry.json"
  telemetry_fallback="$base.capture.telemetry.json"
  start_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  set +e
  (
    cd "$ROOT"
    /usr/bin/time -l env DYLD_LIBRARY_PATH=/usr/lib/swift RECORDIT_FAKE_CAPTURE_FIXTURE="$FIXTURE" "$BIN" \
      --duration-sec "$RUN_DURATION_SEC" \
      --live-chunked \
      --asr-backend whispercpp \
      --asr-model "$MODEL" \
      --input-wav "$input_wav" \
      --out-wav "$out_wav" \
      --benchmark-runs 1 \
      --transcribe-channels mixed-fallback \
      --chunk-window-ms "$CHUNK_WINDOW_MS" \
      --chunk-stride-ms "$CHUNK_STRIDE_MS" \
      --chunk-queue-cap "$CHUNK_QUEUE_CAP" \
      --llm-cleanup \
      --llm-endpoint "$LLM_ENDPOINT" \
      --llm-model "$LLM_MODEL" \
      --llm-timeout-ms "$LLM_TIMEOUT_MS" \
      --llm-max-queue "$LLM_MAX_QUEUE" \
      --llm-retries "$LLM_RETRIES" \
      --out-jsonl "$jsonl" \
      --out-manifest "$manifest"
  ) >"$base.stdout.log" 2>"$base.time.txt"
  exit_code=$?
  set -e

  end_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  real_ms="$(awk '/real/{printf "%.3f", $1*1000; exit}' "$base.time.txt" || true)"
  max_rss_kb="$(awk '/maximum resident set size/{print $1; exit}' "$base.time.txt" || true)"
  real_ms="${real_ms:-0}"
  max_rss_kb="${max_rss_kb:-0}"

  if [[ -f "$manifest" ]] && jq -e . "$manifest" >/dev/null 2>&1; then
    runtime_mode="$(jq -r '.runtime_mode // "unknown"' "$manifest" 2>/dev/null || echo unknown)"
    live_chunked="$(jq -r '.live_config.live_chunked // false' "$manifest" 2>/dev/null || echo false)"
    wall_ms_p95="$(jq -r '.benchmark.wall_ms_p95 // 0' "$manifest" 2>/dev/null || echo 0)"
    wall_ms_p50="$(jq -r '.benchmark.wall_ms_p50 // 0' "$manifest" 2>/dev/null || echo 0)"
    out_wav_materialized="$(jq -r '.out_wav_materialized // false' "$manifest" 2>/dev/null || echo false)"
    out_wav_bytes="$(jq -r '.out_wav_bytes // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_queue_enabled="$(jq -r '.chunk_queue.enabled // false' "$manifest" 2>/dev/null || echo false)"
    chunk_max_queue="$(jq -r '.chunk_queue.max_queue // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_submitted="$(jq -r '.chunk_queue.submitted // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_enqueued="$(jq -r '.chunk_queue.enqueued // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_dropped_oldest="$(jq -r '.chunk_queue.dropped_oldest // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_processed="$(jq -r '.chunk_queue.processed // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_pending="$(jq -r '.chunk_queue.pending // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_high_water="$(jq -r '.chunk_queue.high_water // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_drain_completed="$(jq -r '.chunk_queue.drain_completed // false' "$manifest" 2>/dev/null || echo false)"
    chunk_lag_sample_count="$(jq -r '.chunk_queue.lag_sample_count // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_lag_p50_ms="$(jq -r '.chunk_queue.lag_p50_ms // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_lag_p95_ms="$(jq -r '.chunk_queue.lag_p95_ms // 0' "$manifest" 2>/dev/null || echo 0)"
    chunk_lag_max_ms="$(jq -r '.chunk_queue.lag_max_ms // 0' "$manifest" 2>/dev/null || echo 0)"
    trust_notice_count="$(jq -r '.trust.notice_count // 0' "$manifest" 2>/dev/null || echo 0)"
    degradation_event_count="$(jq -r '(.degradation_events // []) | length' "$manifest" 2>/dev/null || echo 0)"
    reconciliation_applied="$(jq -r '((.degradation_events // []) | any(.code == "reconciliation_applied_after_backpressure"))' "$manifest" 2>/dev/null || echo false)"
    cleanup_dropped_queue_full="$(jq -r '.cleanup_queue.dropped_queue_full // 0' "$manifest" 2>/dev/null || echo 0)"
    cleanup_failed="$(jq -r '.cleanup_queue.failed // 0' "$manifest" 2>/dev/null || echo 0)"
    cleanup_timed_out="$(jq -r '.cleanup_queue.timed_out // 0' "$manifest" 2>/dev/null || echo 0)"
    mode_requested="$(jq -r '.channel_mode_requested // "unknown"' "$manifest" 2>/dev/null || echo unknown)"
    mode_active="$(jq -r '.channel_mode // "unknown"' "$manifest" 2>/dev/null || echo unknown)"
  else
    runtime_mode=unknown
    live_chunked=false
    wall_ms_p95=0
    wall_ms_p50=0
    out_wav_materialized=false
    out_wav_bytes=0
    chunk_queue_enabled=false
    chunk_max_queue=0
    chunk_submitted=0
    chunk_enqueued=0
    chunk_dropped_oldest=0
    chunk_processed=0
    chunk_pending=0
    chunk_high_water=0
    chunk_drain_completed=false
    chunk_lag_sample_count=0
    chunk_lag_p50_ms=0
    chunk_lag_p95_ms=0
    chunk_lag_max_ms=0
    trust_notice_count=0
    degradation_event_count=0
    reconciliation_applied=false
    cleanup_dropped_queue_full=0
    cleanup_failed=0
    cleanup_timed_out=0
    mode_requested=unknown
    mode_active=unknown
  fi
  telemetry="$telemetry_primary"
  if [[ ! -f "$telemetry" && -f "$telemetry_fallback" ]]; then
    telemetry="$telemetry_fallback"
  fi
  if [[ -f "$telemetry" ]] && jq -e . "$telemetry" >/dev/null 2>&1; then
    capture_restart_count="$(jq -r '.restart_count // 0' "$telemetry" 2>/dev/null || echo 0)"
    capture_telemetry_readable=true
  else
    capture_restart_count=-1
    capture_telemetry_readable=false
  fi

  echo "$run,$start_utc,$end_utc,$exit_code,$real_ms,$max_rss_kb,$runtime_mode,$live_chunked,$mode_requested,$mode_active,$out_wav_materialized,$out_wav_bytes,$chunk_queue_enabled,$chunk_max_queue,$chunk_submitted,$chunk_enqueued,$chunk_dropped_oldest,$chunk_processed,$chunk_pending,$chunk_high_water,$chunk_drain_completed,$chunk_lag_sample_count,$chunk_lag_p50_ms,$chunk_lag_p95_ms,$chunk_lag_max_ms,$trust_notice_count,$degradation_event_count,$reconciliation_applied,$capture_restart_count,$capture_telemetry_readable,$wall_ms_p95,$wall_ms_p50,$cleanup_dropped_queue_full,$cleanup_failed,$cleanup_timed_out" >>"$OUT_DIR/runs.csv"
done

echo "GATE_D_OUT=$OUT_DIR"
