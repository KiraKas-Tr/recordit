#!/usr/bin/env bash
set -euo pipefail

evidence_timestamp() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

evidence_write_metadata_json() {
  local output_path="$1"
  local scenario_id="$2"
  local artifact_track="$3"
  local evidence_root="$4"
  local logs_dir="$5"
  local artifacts_dir="$6"
  local summary_path="$7"
  local status_path="$8"
  local script_path="$9"
  local summary_json="${10:-}"
  local status_json="${11:-}"

  python3 - "$output_path" "$scenario_id" "$artifact_track" "$evidence_root" "$logs_dir" "$artifacts_dir" "$summary_path" "$status_path" "$script_path" "$summary_json" "$status_json" <<'PY'
import json
import sys
from pathlib import Path

output_path = Path(sys.argv[1])
summary_path = sys.argv[7]
status_path = sys.argv[8]
payload = {
    "schema_version": 1,
    "scenario_id": sys.argv[2],
    "artifact_track": sys.argv[3],
    "generated_at_utc": __import__("datetime").datetime.utcnow().replace(microsecond=0).isoformat() + "Z",
    "evidence_root": sys.argv[4],
    "logs_dir": sys.argv[5],
    "artifacts_dir": sys.argv[6],
    "summary_path": summary_path,
    "status_path": status_path,
    "summary_csv": summary_path if summary_path.endswith('.csv') else "",
    "status_csv": status_path if status_path.endswith('.csv') else "",
    "summary_json": sys.argv[10],
    "status_json": sys.argv[11],
    "script_path": sys.argv[9],
}
output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

evidence_csv_kv_to_json() {
  local csv_path="$1"
  local output_path="$2"

  python3 - "$csv_path" "$output_path" <<'PY'
import csv
import json
import sys
from pathlib import Path

csv_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
payload = {}

if csv_path.is_file():
    with csv_path.open(newline="", encoding="utf-8") as handle:
        reader = csv.reader(handle)
        for idx, row in enumerate(reader):
            if not row:
                continue
            key = row[0].strip()
            value = "" if len(row) < 2 else row[1]
            if idx == 0 and key.lower() == "key" and str(value).strip().lower() == "value":
                continue
            payload[key] = value

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

evidence_csv_rows_to_json() {
  local csv_path="$1"
  local output_path="$2"

  python3 - "$csv_path" "$output_path" <<'PY'
import csv
import json
import sys
from pathlib import Path

csv_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
rows = []

if csv_path.is_file():
    with csv_path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        rows = list(reader)

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

evidence_kv_text_to_json() {
  local text_path="$1"
  local output_path="$2"

  python3 - "$text_path" "$output_path" <<'PY'
import json
import sys
from pathlib import Path

text_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
payload = {}

if text_path.is_file():
    for raw in text_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith('#') or '=' not in line:
            continue
        key, value = line.split('=', 1)
        payload[key.strip()] = value.strip()

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}
