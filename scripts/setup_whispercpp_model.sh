#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"

MODEL_URL_DEFAULT="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"
MODEL_DEST_DEFAULT="$ROOT/artifacts/bench/models/whispercpp/ggml-tiny.en.bin"

MODEL_URL="$MODEL_URL_DEFAULT"
MODEL_DEST="$MODEL_DEST_DEFAULT"
FORCE=false

usage() {
  cat <<'EOF'
Usage: setup_whispercpp_model.sh [--dest PATH] [--url URL] [--force]

Downloads the default whispercpp tiny model into the deterministic repo path used
by transcribe-live defaults.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest)
      MODEL_DEST="$2"
      shift 2
      ;;
    --url)
      MODEL_URL="$2"
      shift 2
      ;;
    --force)
      FORCE=true
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

if [[ "$MODEL_DEST" != /* ]]; then
  MODEL_DEST="$ROOT/$MODEL_DEST"
fi

if [[ -f "$MODEL_DEST" && "$FORCE" != "true" ]]; then
  if [[ ! -r "$MODEL_DEST" ]]; then
    echo "model exists but is not readable: $MODEL_DEST" >&2
    echo "remediation: fix file permissions or rerun with --force" >&2
    exit 1
  fi
  if [[ ! -s "$MODEL_DEST" ]]; then
    echo "model exists but is empty: $MODEL_DEST" >&2
    echo "remediation: rerun with --force to replace it" >&2
    exit 1
  fi
  echo "whispercpp model already present: $MODEL_DEST"
  echo "sha256=$(shasum -a 256 "$MODEL_DEST" | awk '{print $1}')"
  echo "bytes=$(wc -c < "$MODEL_DEST" | tr -d ' ')"
  exit 0
fi

mkdir -p "$(dirname "$MODEL_DEST")"
tmp_path="$MODEL_DEST.part.$$"
trap 'rm -f "$tmp_path"' EXIT

echo "downloading whispercpp model:"
echo "  url:  $MODEL_URL"
echo "  dest: $MODEL_DEST"

curl -fL --retry 3 --retry-delay 1 --connect-timeout 15 -o "$tmp_path" "$MODEL_URL"

if [[ ! -s "$tmp_path" ]]; then
  echo "download failed or produced empty file: $tmp_path" >&2
  exit 1
fi

mv "$tmp_path" "$MODEL_DEST"

if [[ ! -r "$MODEL_DEST" ]]; then
  echo "downloaded model is not readable: $MODEL_DEST" >&2
  exit 1
fi

echo "whispercpp model ready: $MODEL_DEST"
echo "sha256=$(shasum -a 256 "$MODEL_DEST" | awk '{print $1}')"
echo "bytes=$(wc -c < "$MODEL_DEST" | tr -d ' ')"
