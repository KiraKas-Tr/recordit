#!/usr/bin/env python3

from __future__ import annotations

import csv
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "gate_packaged_readiness_parity_summary.py"
CONTRACT = ROOT / "contracts" / "readiness-contract-ids.v1.json"

REQUIRED_SCENARIOS = [
    "missing-permission",
    "no-display",
    "runtime-preflight-failure",
    "fully-ready",
    "live-blocked-record-allowed-fallback",
]


def build_manifest(path: Path, failing_ids: set[str], overall_status: str) -> None:
    checks = []
    for check_id in [
        "model_path",
        "out_wav",
        "out_jsonl",
        "out_manifest",
        "sample_rate",
        "screen_capture_access",
        "display_availability",
        "microphone_access",
        "backend_runtime",
    ]:
        checks.append(
            {
                "id": check_id,
                "status": "FAIL" if check_id in failing_ids else "PASS",
                "detail": f"{check_id} detail",
                "remediation": "",
            }
        )

    payload = {
        "schema_version": "1",
        "kind": "transcribe-live-preflight",
        "overall_status": overall_status,
        "checks": checks,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_meta(
    path: Path,
    *,
    scenario_id: str,
    expected_domain: str,
    expected_blocking_ids: list[str],
    can_without_ack: bool,
    can_with_ack: bool,
    record_only: bool,
    expected_overall_status: str,
) -> None:
    payload = {
        "scenario_id": scenario_id,
        "scenario_mode": "fixture",
        "expected_primary_blocking_domain": expected_domain,
        "expected_blocking_ids": expected_blocking_ids,
        "expected_can_proceed_without_ack": can_without_ack,
        "expected_can_proceed_with_ack": can_with_ack,
        "expected_record_only_fallback_eligible": record_only,
        "expected_overall_status": expected_overall_status,
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_execution(path: Path, manifest_path: Path) -> None:
    payload = {
        "scenario_id": manifest_path.parent.name,
        "exit_code": 0,
        "stdout_log": str(manifest_path.parent / "stdout.log"),
        "stderr_log": str(manifest_path.parent / "stderr.log"),
        "preflight_manifest_path": str(manifest_path),
        "started_at_utc": "2026-03-07T00:00:00Z",
        "ended_at_utc": "2026-03-07T00:00:01Z",
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class PackagedReadinessParitySummaryTests(unittest.TestCase):
    def _create_scenarios(self, root: Path, *, omit: str | None = None) -> Path:
        scenarios = root / "scenarios"
        scenarios.mkdir(parents=True, exist_ok=True)

        fixtures = {
            "missing-permission": {
                "fails": {"screen_capture_access", "microphone_access"},
                "overall": "FAIL",
                "expected_domain": "tcc_capture",
                "expected_blocking_ids": ["microphone_access", "screen_capture_access"],
                "can_without_ack": False,
                "can_with_ack": False,
                "record_only": False,
            },
            "no-display": {
                "fails": {"display_availability"},
                "overall": "FAIL",
                "expected_domain": "tcc_capture",
                "expected_blocking_ids": ["display_availability"],
                "can_without_ack": False,
                "can_with_ack": False,
                "record_only": False,
            },
            "runtime-preflight-failure": {
                "fails": {"out_wav"},
                "overall": "FAIL",
                "expected_domain": "runtime_preflight",
                "expected_blocking_ids": ["out_wav"],
                "can_without_ack": False,
                "can_with_ack": False,
                "record_only": False,
            },
            "fully-ready": {
                "fails": set(),
                "overall": "PASS",
                "expected_domain": "none",
                "expected_blocking_ids": [],
                "can_without_ack": True,
                "can_with_ack": True,
                "record_only": False,
            },
            "live-blocked-record-allowed-fallback": {
                "fails": {"model_path"},
                "overall": "FAIL",
                "expected_domain": "backend_model",
                "expected_blocking_ids": ["model_path"],
                "can_without_ack": False,
                "can_with_ack": False,
                "record_only": True,
            },
        }

        for scenario_id, data in fixtures.items():
            if omit and scenario_id == omit:
                continue
            scenario_dir = scenarios / scenario_id
            scenario_dir.mkdir(parents=True, exist_ok=True)
            manifest = scenario_dir / "preflight.manifest.json"
            build_manifest(manifest, data["fails"], data["overall"])
            write_meta(
                scenario_dir / "scenario_meta.json",
                scenario_id=scenario_id,
                expected_domain=data["expected_domain"],
                expected_blocking_ids=data["expected_blocking_ids"],
                can_without_ack=data["can_without_ack"],
                can_with_ack=data["can_with_ack"],
                record_only=data["record_only"],
                expected_overall_status=data["overall"],
            )
            write_execution(scenario_dir / "execution.json", manifest)
            (scenario_dir / "stdout.log").write_text("stdout\n", encoding="utf-8")
            (scenario_dir / "stderr.log").write_text("stderr\n", encoding="utf-8")

        return scenarios

    def _run(self, scenarios_root: Path, out_root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "python3",
                str(SCRIPT),
                "--scenarios-root",
                str(scenarios_root),
                "--contract-path",
                str(CONTRACT),
                "--summary-csv",
                str(out_root / "summary.csv"),
                "--summary-json",
                str(out_root / "summary.json"),
                "--status-path",
                str(out_root / "status.txt"),
            ],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_passes_for_expected_matrix(self) -> None:
        with tempfile.TemporaryDirectory(prefix="gate_packaged_readiness_summary_pass_") as tmp:
            root = Path(tmp)
            scenarios = self._create_scenarios(root)
            out = root / "out"

            proc = self._run(scenarios, out)
            self.assertEqual(proc.returncode, 0, msg=proc.stderr)

            with (out / "summary.csv").open(newline="", encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle))
            self.assertEqual({row["scenario_id"] for row in rows}, set(REQUIRED_SCENARIOS))
            self.assertTrue(all(row["status"] == "pass" for row in rows))

            status = (out / "status.txt").read_text(encoding="utf-8")
            self.assertIn("status=pass", status)

    def test_fails_when_required_scenario_missing(self) -> None:
        with tempfile.TemporaryDirectory(prefix="gate_packaged_readiness_summary_missing_") as tmp:
            root = Path(tmp)
            scenarios = self._create_scenarios(root, omit="no-display")
            out = root / "out"

            proc = self._run(scenarios, out)
            self.assertNotEqual(proc.returncode, 0)

            payload = json.loads((out / "summary.json").read_text(encoding="utf-8"))
            self.assertIn("no-display", payload["missing_required_scenarios"])
            self.assertEqual(payload["status"], "fail")


if __name__ == "__main__":
    unittest.main()
