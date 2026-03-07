#!/usr/bin/env python3
"""Summarize packaged readiness-parity scenarios into contract-driven gating decisions."""

from __future__ import annotations

import argparse
import csv
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

DEFAULT_CONTRACT_PATH = Path("contracts/readiness-contract-ids.v1.json")

REQUIRED_SCENARIOS = (
    "missing-permission",
    "no-display",
    "runtime-preflight-failure",
    "fully-ready",
    "live-blocked-record-allowed-fallback",
)

PRIMARY_BLOCKING_ORDER = (
    "tcc_capture",
    "backend_model",
    "runtime_preflight",
    "backend_runtime",
    "unknown",
)


class ScenarioError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenarios-root", required=True, type=Path)
    parser.add_argument("--contract-path", default=DEFAULT_CONTRACT_PATH, type=Path)
    parser.add_argument("--summary-csv", required=True, type=Path)
    parser.add_argument("--summary-json", required=True, type=Path)
    parser.add_argument("--status-path", required=True, type=Path)
    parser.add_argument(
        "--required-scenarios",
        nargs="*",
        default=list(REQUIRED_SCENARIOS),
        help="Required scenario IDs that must exist in every matrix run.",
    )
    return parser.parse_args()


def now_utc() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def parse_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    text = str(value or "").strip().lower()
    return text in {"1", "true", "yes", "y"}


def load_json(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    return payload if isinstance(payload, dict) else {}


def load_contract(contract_path: Path) -> dict[str, dict[str, str]]:
    payload = load_json(contract_path)
    rows = payload.get("preflight_check_ids")
    if not isinstance(rows, list):
        raise ScenarioError(f"contract missing preflight_check_ids: {contract_path}")

    mapping: dict[str, dict[str, str]] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        check_id = str(row.get("id") or "").strip()
        if not check_id:
            continue
        mapping[check_id] = {
            "domain": str(row.get("domain") or "unknown").strip() or "unknown",
            "class": str(row.get("class") or "informational").strip() or "informational",
        }

    if not mapping:
        raise ScenarioError(f"contract contains no usable preflight IDs: {contract_path}")
    return mapping


def normalize_status(value: Any) -> str:
    text = str(value or "").strip().upper()
    if text in {"PASS", "WARN", "FAIL"}:
        return text
    return "UNKNOWN"


def evaluate_manifest(
    manifest: dict[str, Any],
    contract: dict[str, dict[str, str]],
) -> dict[str, Any]:
    checks = manifest.get("checks")
    if not isinstance(checks, list):
        checks = []

    mapped_checks: list[dict[str, Any]] = []
    blocking_failures: list[dict[str, Any]] = []
    warning_continuations: list[dict[str, Any]] = []
    unknown_check_ids: list[str] = []

    for row in checks:
        if not isinstance(row, dict):
            continue
        check_id = str(row.get("id") or "").strip()
        if not check_id:
            continue

        status = normalize_status(row.get("status"))
        contract_row = contract.get(check_id)
        if contract_row is None:
            policy = "informational"
            domain = "unknown"
            is_known = False
            unknown_check_ids.append(check_id)
        else:
            domain = contract_row["domain"]
            class_name = contract_row["class"]
            if class_name == "blocking":
                policy = "blockOnFail"
            elif class_name == "warn_ack_required":
                policy = "warnRequiresAcknowledgement"
            else:
                policy = "informational"
            is_known = True

        mapped = {
            "id": check_id,
            "status": status,
            "detail": str(row.get("detail") or ""),
            "remediation": str(row.get("remediation") or ""),
            "policy": policy,
            "domain": domain,
            "is_known_contract_id": is_known,
        }
        mapped_checks.append(mapped)

        if policy == "blockOnFail":
            if status == "FAIL":
                blocking_failures.append(mapped)
        elif policy == "warnRequiresAcknowledgement":
            if status == "FAIL" and domain == "backend_runtime":
                blocking_failures.append(mapped)
            elif status != "PASS":
                warning_continuations.append(mapped)

    blocking_by_domain: dict[str, list[str]] = {}
    for row in blocking_failures:
        blocking_by_domain.setdefault(row["domain"], []).append(row["id"])

    primary_blocking_domain = "none"
    for domain in PRIMARY_BLOCKING_ORDER:
        if blocking_by_domain.get(domain):
            primary_blocking_domain = domain
            break
    if primary_blocking_domain == "none" and blocking_failures:
        primary_blocking_domain = "unknown"

    has_live_specific_only_blockers = bool(blocking_by_domain.get("backend_model") or blocking_by_domain.get("backend_runtime"))
    has_other_blockers = any(
        domain not in {"backend_model", "backend_runtime"} and ids
        for domain, ids in blocking_by_domain.items()
    )

    can_proceed_without_ack = not blocking_failures and not warning_continuations
    can_proceed_with_ack = not blocking_failures
    record_only_fallback_eligible = has_live_specific_only_blockers and not has_other_blockers

    return {
        "mapped_checks": mapped_checks,
        "blocking_failures": blocking_failures,
        "warning_continuations": warning_continuations,
        "unknown_check_ids": sorted(set(unknown_check_ids)),
        "blocking_ids": sorted({row["id"] for row in blocking_failures}),
        "warning_ids": sorted({row["id"] for row in warning_continuations}),
        "primary_blocking_domain": primary_blocking_domain,
        "can_proceed_without_ack": can_proceed_without_ack,
        "can_proceed_with_ack": can_proceed_with_ack,
        "record_only_fallback_eligible": record_only_fallback_eligible,
    }


def scenario_rows(
    scenarios_root: Path,
    contract: dict[str, dict[str, str]],
) -> tuple[list[dict[str, str]], list[dict[str, Any]]]:
    rows: list[dict[str, str]] = []
    diagnostics: list[dict[str, Any]] = []

    if not scenarios_root.is_dir():
        raise ScenarioError(f"scenarios root does not exist: {scenarios_root}")

    for scenario_dir in sorted(path for path in scenarios_root.iterdir() if path.is_dir()):
        scenario_id = scenario_dir.name
        meta = load_json(scenario_dir / "scenario_meta.json")
        execution = load_json(scenario_dir / "execution.json")

        manifest_path = Path(str(execution.get("preflight_manifest_path") or scenario_dir / "preflight.manifest.json"))
        if not manifest_path.is_absolute():
            manifest_path = (scenario_dir / manifest_path).resolve(strict=False)
        manifest = load_json(manifest_path)

        manifest_present = bool(manifest)
        overall_status = str(manifest.get("overall_status") or "").strip().upper() if manifest else "MISSING"

        evaluation: dict[str, Any]
        if manifest_present:
            evaluation = evaluate_manifest(manifest, contract)
        else:
            evaluation = {
                "mapped_checks": [],
                "blocking_failures": [],
                "warning_continuations": [],
                "unknown_check_ids": [],
                "blocking_ids": [],
                "warning_ids": [],
                "primary_blocking_domain": "missing_manifest",
                "can_proceed_without_ack": False,
                "can_proceed_with_ack": False,
                "record_only_fallback_eligible": False,
            }

        expected_primary_domain = str(meta.get("expected_primary_blocking_domain") or "none")
        expected_blocking_ids_raw = meta.get("expected_blocking_ids") or []
        if isinstance(expected_blocking_ids_raw, str):
            expected_blocking_ids = [token for token in (item.strip() for item in expected_blocking_ids_raw.split("|")) if token]
        elif isinstance(expected_blocking_ids_raw, list):
            expected_blocking_ids = [str(item).strip() for item in expected_blocking_ids_raw if str(item).strip()]
        else:
            expected_blocking_ids = []

        expected_can_without_ack = parse_bool(meta.get("expected_can_proceed_without_ack"))
        expected_can_with_ack = parse_bool(meta.get("expected_can_proceed_with_ack"))
        expected_record_only = parse_bool(meta.get("expected_record_only_fallback_eligible"))
        expected_overall_status = str(meta.get("expected_overall_status") or "")

        checks = [manifest_present]
        checks.append(evaluation["primary_blocking_domain"] == expected_primary_domain)
        checks.append(evaluation["can_proceed_without_ack"] == expected_can_without_ack)
        checks.append(evaluation["can_proceed_with_ack"] == expected_can_with_ack)
        checks.append(evaluation["record_only_fallback_eligible"] == expected_record_only)
        checks.append(sorted(evaluation["blocking_ids"]) == sorted(expected_blocking_ids))
        if expected_overall_status:
            checks.append(overall_status == expected_overall_status)

        status = "pass" if all(checks) else "fail"

        exit_code = execution.get("exit_code")
        if exit_code is None or str(exit_code).strip() == "":
            exit_code_text = ""
        else:
            try:
                exit_code_text = str(int(exit_code))
            except ValueError:
                exit_code_text = str(exit_code)

        diagnostics_payload = {
            "expected_primary_blocking_domain": expected_primary_domain,
            "observed_primary_blocking_domain": evaluation["primary_blocking_domain"],
            "expected_blocking_ids": sorted(expected_blocking_ids),
            "observed_blocking_ids": sorted(evaluation["blocking_ids"]),
            "expected_can_proceed_without_ack": expected_can_without_ack,
            "observed_can_proceed_without_ack": evaluation["can_proceed_without_ack"],
            "expected_can_proceed_with_ack": expected_can_with_ack,
            "observed_can_proceed_with_ack": evaluation["can_proceed_with_ack"],
            "expected_record_only_fallback_eligible": expected_record_only,
            "observed_record_only_fallback_eligible": evaluation["record_only_fallback_eligible"],
            "unknown_check_ids": evaluation["unknown_check_ids"],
            "overall_status": overall_status,
            "expected_overall_status": expected_overall_status,
        }

        rows.append(
            {
                "scenario_id": scenario_id,
                "status": status,
                "scenario_mode": str(meta.get("scenario_mode") or "unknown"),
                "overall_status": overall_status,
                "expected_primary_blocking_domain": expected_primary_domain,
                "primary_blocking_domain": str(evaluation["primary_blocking_domain"]),
                "expected_blocking_ids": "|".join(sorted(expected_blocking_ids)),
                "blocking_ids": "|".join(sorted(evaluation["blocking_ids"])),
                "warning_ids": "|".join(sorted(evaluation["warning_ids"])),
                "expected_can_proceed_without_ack": bool_text(expected_can_without_ack),
                "can_proceed_without_ack": bool_text(evaluation["can_proceed_without_ack"]),
                "expected_can_proceed_with_ack": bool_text(expected_can_with_ack),
                "can_proceed_with_ack": bool_text(evaluation["can_proceed_with_ack"]),
                "expected_record_only_fallback_eligible": bool_text(expected_record_only),
                "record_only_fallback_eligible": bool_text(evaluation["record_only_fallback_eligible"]),
                "expected_overall_status": expected_overall_status,
                "exit_code": exit_code_text,
                "manifest_path": str(manifest_path),
                "stdout_log": str(execution.get("stdout_log") or (scenario_dir / "stdout.log")),
                "stderr_log": str(execution.get("stderr_log") or (scenario_dir / "stderr.log")),
                "started_at_utc": str(execution.get("started_at_utc") or ""),
                "ended_at_utc": str(execution.get("ended_at_utc") or ""),
                "diagnostics": json.dumps(diagnostics_payload, sort_keys=True),
            }
        )

        diagnostics.append(
            {
                "scenario_id": scenario_id,
                "status": status,
                "evaluation": evaluation,
                "expected": diagnostics_payload,
                "manifest_path": str(manifest_path),
            }
        )

    return rows, diagnostics


def write_csv(path: Path, rows: list[dict[str, str]]) -> None:
    fieldnames = [
        "scenario_id",
        "status",
        "scenario_mode",
        "overall_status",
        "expected_primary_blocking_domain",
        "primary_blocking_domain",
        "expected_blocking_ids",
        "blocking_ids",
        "warning_ids",
        "expected_can_proceed_without_ack",
        "can_proceed_without_ack",
        "expected_can_proceed_with_ack",
        "can_proceed_with_ack",
        "expected_record_only_fallback_eligible",
        "record_only_fallback_eligible",
        "expected_overall_status",
        "exit_code",
        "manifest_path",
        "stdout_log",
        "stderr_log",
        "started_at_utc",
        "ended_at_utc",
        "diagnostics",
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def write_json(
    path: Path,
    rows: list[dict[str, str]],
    *,
    required_scenarios: list[str],
    missing_required: list[str],
    status: str,
    generated_at_utc: str,
    contract_path: Path,
    scenario_diagnostics: list[dict[str, Any]],
) -> None:
    payload = {
        "generated_at_utc": generated_at_utc,
        "status": status,
        "required_scenarios": required_scenarios,
        "missing_required_scenarios": missing_required,
        "total_scenarios": len(rows),
        "failed_scenarios": [row["scenario_id"] for row in rows if row.get("status") != "pass"],
        "contract_path": str(contract_path),
        "rows": rows,
        "scenario_diagnostics": scenario_diagnostics,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_status(
    path: Path,
    *,
    status: str,
    generated_at_utc: str,
    total_scenarios: int,
    failed_count: int,
    missing_required: list[str],
    summary_csv: Path,
    summary_json: Path,
) -> None:
    lines = [
        f"status={status}",
        f"generated_at_utc={generated_at_utc}",
        f"total_scenarios={total_scenarios}",
        f"failed_scenarios={failed_count}",
        f"missing_required_scenarios={'|'.join(missing_required)}",
        f"summary_csv={summary_csv}",
        f"summary_json={summary_json}",
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    args = parse_args()

    contract_path = args.contract_path
    if not contract_path.is_absolute():
        contract_path = (Path.cwd() / contract_path).resolve(strict=False)

    contract = load_contract(contract_path)
    rows, scenario_diagnostics = scenario_rows(args.scenarios_root, contract)
    by_id = {row["scenario_id"]: row for row in rows}

    required_scenarios = [scenario.strip() for scenario in args.required_scenarios if scenario.strip()]
    missing_required = [scenario for scenario in required_scenarios if scenario not in by_id]

    failed_count = sum(1 for row in rows if row.get("status") != "pass")
    status = "pass" if failed_count == 0 and not missing_required and rows else "fail"
    generated_at_utc = now_utc()

    write_csv(args.summary_csv, rows)
    write_json(
        args.summary_json,
        rows,
        required_scenarios=required_scenarios,
        missing_required=missing_required,
        status=status,
        generated_at_utc=generated_at_utc,
        contract_path=contract_path,
        scenario_diagnostics=scenario_diagnostics,
    )
    write_status(
        args.status_path,
        status=status,
        generated_at_utc=generated_at_utc,
        total_scenarios=len(rows),
        failed_count=failed_count,
        missing_required=missing_required,
        summary_csv=args.summary_csv,
        summary_json=args.summary_json,
    )

    if status != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
