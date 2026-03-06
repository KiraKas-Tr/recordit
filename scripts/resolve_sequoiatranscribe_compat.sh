#!/usr/bin/env bash
set -euo pipefail

ROOT_DEFAULT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${ROOT:-$ROOT_DEFAULT}"
EMIT_WARNING="${EMIT_WARNING:-1}"
SUNSET_TARGET="${SUNSET_TARGET:-2026-Q3}"

usage() {
  cat <<USAGE
Usage: $0 [options]

Resolve the executable path for legacy automation that still expects
SequoiaTranscribe compatibility entrypoints.

Resolution order:
  1. \$SEQUOIA_TRANSCRIBE_COMPAT_BIN (if executable)
  2. dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe

Options:
  --root PATH     Project root (default: script-relative root)
  --no-warning    Suppress compatibility warning output
  -h, --help      Show this help text
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      ROOT="$2"
      shift 2
      ;;
    --no-warning)
      EMIT_WARNING=0
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

legacy_bin="$ROOT/dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe"
override_bin="${SEQUOIA_TRANSCRIBE_COMPAT_BIN:-}"

resolved_bin=""
resolved_source=""

if [[ -n "$override_bin" && -x "$override_bin" ]]; then
  resolved_bin="$override_bin"
  resolved_source="SEQUOIA_TRANSCRIBE_COMPAT_BIN"
elif [[ -x "$legacy_bin" ]]; then
  resolved_bin="$legacy_bin"
  resolved_source="legacy_default"
fi

if [[ -z "$resolved_bin" ]]; then
  echo "error: could not resolve SequoiaTranscribe compatibility executable." >&2
  echo "checked: $legacy_bin" >&2
  echo "hint: run \`make sign-transcribe\` or set SEQUOIA_TRANSCRIBE_COMPAT_BIN to an executable path." >&2
  exit 1
fi

if [[ "$EMIT_WARNING" == "1" ]]; then
  echo "compat_warning=SequoiaTranscribe_legacy_path_in_use" >&2
  echo "compat_source=$resolved_source" >&2
  echo "compat_sunset_target=$SUNSET_TARGET" >&2
  echo "compat_hint=Prefer Recordit.app as default user path; keep compatibility invocation only for legacy automation/incident workflows." >&2
fi

echo "$resolved_bin"
