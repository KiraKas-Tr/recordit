#!/usr/bin/env bash
set -euo pipefail

ROOT="${SRCROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
MANIFEST_PATH="$ROOT/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  echo "error: Cargo manifest not found at $MANIFEST_PATH" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required" >&2
  exit 2
fi

resolve_cargo_bin() {
  if [[ -n "${CARGO_BIN:-}" ]]; then
    if [[ -x "${CARGO_BIN}" ]]; then
      printf '%s\n' "${CARGO_BIN}"
      return 0
    fi
    echo "error: CARGO_BIN is set but not executable: ${CARGO_BIN}" >&2
    return 1
  fi

  local path_candidate
  path_candidate="$(command -v cargo 2>/dev/null || true)"
  if [[ -n "$path_candidate" && -x "$path_candidate" ]]; then
    printf '%s\n' "$path_candidate"
    return 0
  fi

  local fallback_candidates=(
    "$HOME/.cargo/bin/cargo"
    "/opt/homebrew/bin/cargo"
    "/usr/local/bin/cargo"
  )
  local candidate
  for candidate in "${fallback_candidates[@]}"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  echo "error: could not locate cargo. Install Rust toolchain or set CARGO_BIN to an absolute cargo path." >&2
  return 1
}

configuration="${RECORDIT_RUNTIME_CONFIGURATION:-${CONFIGURATION:-Release}}"
cargo_profile="debug"
if [[ "$configuration" == "Release" ]]; then
  cargo_profile="release"
fi

runtime_input_dir="${RECORDIT_RUNTIME_INPUT_DIR:-$ROOT/.build/recordit-runtime-inputs/$configuration}"
runtime_bin_dir="$runtime_input_dir/runtime/bin"
runtime_model_dir="$runtime_input_dir/runtime/models/whispercpp"
staged_model_path="$runtime_model_dir/ggml-tiny.en.bin"
default_model_src="${RECORDIT_DEFAULT_WHISPERCPP_MODEL:-$ROOT/artifacts/bench/models/whispercpp/ggml-tiny.en.bin}"

cargo_bin="$(resolve_cargo_bin)"

echo "[runtime-handoff][rust-build] configuration=$configuration profile=$cargo_profile"
echo "[runtime-handoff][rust-build] output_dir=$runtime_input_dir"
cargo_args=(build --manifest-path "$MANIFEST_PATH" --bin recordit --bin sequoia_capture)
if [[ "$cargo_profile" == "release" ]]; then
  cargo_args+=(--release)
fi
"$cargo_bin" "${cargo_args[@]}"

target_root="${CARGO_TARGET_DIR:-$ROOT/target}"
recordit_src="$target_root/$cargo_profile/recordit"
capture_src="$target_root/$cargo_profile/sequoia_capture"

if [[ ! -x "$recordit_src" ]]; then
  echo "error: recordit binary missing after rust build: $recordit_src" >&2
  exit 1
fi
if [[ ! -x "$capture_src" ]]; then
  echo "error: sequoia_capture binary missing after rust build: $capture_src" >&2
  exit 1
fi

mkdir -p "$runtime_bin_dir"
rm -f "$runtime_bin_dir/recordit" "$runtime_bin_dir/sequoia_capture"
install -m 755 "$recordit_src" "$runtime_bin_dir/recordit"
install -m 755 "$capture_src" "$runtime_bin_dir/sequoia_capture"
echo "[runtime-handoff][rust-build] staged runtime binaries into $runtime_bin_dir"

if [[ -f "$default_model_src" ]]; then
  mkdir -p "$runtime_model_dir"
  if [[ -e "$staged_model_path" && "$default_model_src" -ef "$staged_model_path" ]]; then
    echo "[runtime-handoff][rust-build] default whispercpp model already staged at $staged_model_path"
  else
    rm -f "$staged_model_path"
    install -m 644 "$default_model_src" "$staged_model_path"
    echo "[runtime-handoff][rust-build] staged default whispercpp model into $runtime_model_dir"
  fi
else
  rm -f "$staged_model_path"
  echo "[runtime-handoff][rust-build] warning: default whispercpp model not found at $default_model_src; live onboarding may require manual model path" >&2
fi

runtime_artifact_manifest="$runtime_input_dir/runtime/artifact-manifest.json"
python3 - "$runtime_input_dir" "$configuration" "$cargo_profile" "$runtime_artifact_manifest" "$runtime_bin_dir/recordit" "$runtime_bin_dir/sequoia_capture" "$staged_model_path" <<'PY_MANIFEST'
from __future__ import annotations

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

runtime_input_dir = Path(sys.argv[1]).resolve()
runtime_root = runtime_input_dir / 'runtime'
configuration = sys.argv[2]
cargo_profile = sys.argv[3]
manifest_path = Path(sys.argv[4])
artifact_specs = [
    ("recordit", Path(sys.argv[5])),
    ("sequoia_capture", Path(sys.argv[6])),
    ("whispercpp_default_model", Path(sys.argv[7])),
]


def digest(path: Path) -> str:
    sha = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            sha.update(chunk)
    return sha.hexdigest()

entries = []
for logical_name, path in artifact_specs:
    if not path.is_file():
        continue
    entries.append(
        {
            "logical_name": logical_name,
            "kind": "model" if logical_name == "whispercpp_default_model" else "binary",
            "path": path.resolve().relative_to(runtime_root).as_posix(),
            "size_bytes": path.stat().st_size,
            "sha256": digest(path),
        }
    )
entries.sort(key=lambda row: row["path"])
manifest = {
    "schema_version": 1,
    "generated_at_utc": datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    "configuration": configuration,
    "cargo_profile": cargo_profile,
    "runtime_input_dir": str(runtime_input_dir),
    "entries": entries,
}
manifest_path.parent.mkdir(parents=True, exist_ok=True)
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding='utf-8')
PY_MANIFEST

echo "[runtime-handoff][rust-build] wrote runtime artifact manifest: $runtime_artifact_manifest"

metadata_file="$runtime_input_dir/runtime_handoff.env"
{
  printf 'RECORDIT_RUNTIME_CONFIGURATION=%q\n' "$configuration"
  printf 'RECORDIT_RUNTIME_INPUT_DIR=%q\n' "$runtime_input_dir"
  printf 'RECORDIT_RUNTIME_BIN_RECORDIT=%q\n' "$runtime_bin_dir/recordit"
  printf 'RECORDIT_RUNTIME_BIN_CAPTURE=%q\n' "$runtime_bin_dir/sequoia_capture"
  printf 'RECORDIT_RUNTIME_MODEL_DEFAULT=%q\n' "$staged_model_path"
  printf 'RECORDIT_RUNTIME_ARTIFACT_MANIFEST=%q\n' "$runtime_artifact_manifest"
} >"$metadata_file"
echo "[runtime-handoff][rust-build] wrote handoff metadata: $metadata_file"
