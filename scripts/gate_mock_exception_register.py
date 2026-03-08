#!/usr/bin/env python3
"""Inventory seam usage and enforce exception-register coverage for critical paths."""

from __future__ import annotations

import argparse
import csv
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

SEAM_ALIASES: dict[str, tuple[str, ...]] = {
    "mock": ("mock", "mockservices", "mocked"),
    "stub": ("stub", "stubs", "stubbed"),
    "fixture": ("fixture", "fixtures", "frozen"),
    "fake_capture": ("fake_capture", "recordit_fake_capture_fixture"),
    "ui_test_mode": ("ui_test_mode", "recordit_ui_test_mode", "ui-test-mode"),
    "preview_di": ("preview_di", "appenvironment.preview()", "preview"),
    "scripted_runtime": ("scripted_runtime", "scripted runtime", "scripted preflight"),
    "runtime_override": ("runtime_override", "runtime overrides", "/usr/bin/true"),
    "temp_filesystem": ("temp_filesystem", "temp-filesystem", "synthetic roots"),
    "packaged_checks": ("packaged_checks", "packaged check", "packaged audit"),
}


@dataclass(frozen=True)
class Violation:
    type: str
    surface_key: str
    detail: str
    exception_id: str = ""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--canonical-matrix-csv",
        type=Path,
        default=Path("docs/bd-39i6-canonical-downstream-matrix.csv"),
    )
    parser.add_argument(
        "--critical-surface-csv",
        type=Path,
        default=Path("docs/bd-39i6-critical-surface-coverage-matrix.csv"),
    )
    parser.add_argument(
        "--exception-register-csv",
        type=Path,
        default=Path("docs/bd-2mbp-critical-path-exception-register.csv"),
    )
    parser.add_argument("--summary-csv", required=True, type=Path)
    parser.add_argument("--status-json", required=False, type=Path)
    parser.add_argument("--policy-mode", choices=["fail", "warn"], default="fail")
    return parser.parse_args()


def read_csv_rows(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        raise SystemExit(f"csv file not found: {path}")
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise SystemExit(f"csv has no header row: {path}")
        return [{k: (v or "").strip() for k, v in row.items()} for row in reader]


def split_tokens(value: str) -> list[str]:
    raw = value.replace(";", "|").replace(",", "|")
    return [token.strip() for token in raw.split("|") if token.strip()]


def canonicalize_seam(token: str) -> str:
    normalized = token.strip().lower().replace("-", "_").replace(" ", "_")
    for seam, aliases in SEAM_ALIASES.items():
        if normalized == seam:
            return seam
        for alias in aliases:
            alias_norm = alias.lower().replace("-", "_").replace(" ", "_")
            if normalized == alias_norm or alias_norm in normalized:
                return seam
    return ""


def parse_iso8601_utc(value: str) -> datetime | None:
    if not value:
        return None
    cleaned = value.strip()
    if cleaned.endswith("Z"):
        cleaned = cleaned[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(cleaned)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def seam_families_from_text_fields(*values: str) -> set[str]:
    families: set[str] = set()
    for value in values:
        for token in split_tokens(value):
            seam = canonicalize_seam(token)
            if seam:
                families.add(seam)
    return families


def index_tracked_exceptions(
    exception_rows: list[dict[str, str]],
) -> dict[str, list[dict[str, str]]]:
    tracked: dict[str, list[dict[str, str]]] = {}
    for row in exception_rows:
        status = row.get("status", "").lower()
        if status not in {"active", "replacement_in_progress"}:
            continue
        surface_key = row.get("surface_key", "")
        if not surface_key:
            continue
        tracked.setdefault(surface_key, []).append(row)
    return tracked


def inventory_layer_seams(
    critical_rows: list[dict[str, str]],
) -> dict[str, dict[str, int]]:
    inventory: dict[str, dict[str, int]] = {}
    for row in critical_rows:
        layer = row.get("layer", "").strip() or "unknown"
        seam_families = seam_families_from_text_fields(
            row.get("realism", ""),
            row.get("simulation_or_bypass", ""),
        )
        if not seam_families:
            continue
        layer_counts = inventory.setdefault(layer, {})
        for seam in seam_families:
            layer_counts[seam] = layer_counts.get(seam, 0) + 1
    return inventory


def analyze(
    canonical_rows: list[dict[str, str]],
    critical_rows: list[dict[str, str]],
    exception_rows: list[dict[str, str]],
) -> tuple[list[Violation], dict[str, dict[str, int]], dict[str, int]]:
    violations: list[Violation] = []
    tracked_index = index_tracked_exceptions(exception_rows)
    now_utc = datetime.now(timezone.utc)

    seam_surface_count = 0
    for row in canonical_rows:
        surface_key = row.get("surface_key", "")
        if not surface_key:
            continue
        seam_families = seam_families_from_text_fields(
            row.get("main_bypass_or_limit", ""),
            row.get("realism_class", ""),
            row.get("gap_status", ""),
        )
        if not seam_families:
            continue
        seam_surface_count += 1
        tracked_rows = tracked_index.get(surface_key, [])
        if not tracked_rows:
            violations.append(
                Violation(
                    type="missing_exception",
                    surface_key=surface_key,
                    detail=f"seam families detected without register row: {sorted(seam_families)}",
                )
            )
            continue

        for exception in tracked_rows:
            exception_id = exception.get("exception_id", "")
            owner_area = exception.get("owner_area", "")
            replacement_bead = exception.get("replacement_bead", "")
            expires_at = parse_iso8601_utc(exception.get("expires_at_utc", ""))

            if not owner_area or not replacement_bead:
                violations.append(
                    Violation(
                        type="missing_metadata",
                        surface_key=surface_key,
                        detail="owner_area or replacement_bead is empty",
                        exception_id=exception_id,
                    )
                )
            if expires_at is None:
                violations.append(
                    Violation(
                        type="invalid_expiry",
                        surface_key=surface_key,
                        detail="expires_at_utc missing or invalid",
                        exception_id=exception_id,
                    )
                )
            elif expires_at < now_utc:
                violations.append(
                    Violation(
                        type="expired_exception",
                        surface_key=surface_key,
                        detail=f"expired at {exception.get('expires_at_utc', '')}",
                        exception_id=exception_id,
                    )
                )

    layer_inventory = inventory_layer_seams(critical_rows)
    metrics = {
        "canonical_rows": len(canonical_rows),
        "critical_rows": len(critical_rows),
        "exception_rows": len(exception_rows),
        "tracked_exception_rows": sum(
            1
            for row in exception_rows
            if row.get("status", "").lower() in {"active", "replacement_in_progress"}
        ),
        "seam_surface_count": seam_surface_count,
    }
    return violations, layer_inventory, metrics


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def write_summary(
    path: Path,
    args: argparse.Namespace,
    gate_pass: bool,
    violations: list[Violation],
    layer_inventory: dict[str, dict[str, int]],
    metrics: dict[str, int],
) -> None:
    by_type: dict[str, int] = {}
    for violation in violations:
        by_type[violation.type] = by_type.get(violation.type, 0) + 1

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["key", "value"])
        writer.writerow(
            ["generated_at_utc", datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")]
        )
        writer.writerow(["artifact_track", "gate_mock_exception_register"])
        writer.writerow(["canonical_matrix_csv", str(args.canonical_matrix_csv)])
        writer.writerow(["critical_surface_csv", str(args.critical_surface_csv)])
        writer.writerow(["exception_register_csv", str(args.exception_register_csv)])
        writer.writerow(["policy_mode", args.policy_mode])
        writer.writerow(["gate_pass", bool_text(gate_pass)])
        writer.writerow(["canonical_rows", metrics["canonical_rows"]])
        writer.writerow(["critical_rows", metrics["critical_rows"]])
        writer.writerow(["exception_rows", metrics["exception_rows"]])
        writer.writerow(["tracked_exception_rows", metrics["tracked_exception_rows"]])
        writer.writerow(["seam_surface_count", metrics["seam_surface_count"]])
        writer.writerow(["violation_count", len(violations)])
        writer.writerow(
            ["missing_exception_count", by_type.get("missing_exception", 0)]
        )
        writer.writerow(["expired_exception_count", by_type.get("expired_exception", 0)])
        writer.writerow(["invalid_expiry_count", by_type.get("invalid_expiry", 0)])
        writer.writerow(["missing_metadata_count", by_type.get("missing_metadata", 0)])
        writer.writerow(
            ["layer_inventory_json", json.dumps(layer_inventory, sort_keys=True)]
        )
        writer.writerow(
            [
                "violations",
                "|".join(
                    f"{v.type}:{v.surface_key}:{v.exception_id}" for v in violations
                ),
            ]
        )


def write_status_json(
    path: Path,
    args: argparse.Namespace,
    gate_pass: bool,
    violations: list[Violation],
    layer_inventory: dict[str, dict[str, int]],
    metrics: dict[str, int],
) -> None:
    payload = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "artifact_track": "gate_mock_exception_register",
        "policy_mode": args.policy_mode,
        "gate_pass": gate_pass,
        "metrics": metrics,
        "layer_inventory": layer_inventory,
        "violation_count": len(violations),
        "violations": [
            {
                "type": v.type,
                "surface_key": v.surface_key,
                "detail": v.detail,
                "exception_id": v.exception_id,
            }
            for v in violations
        ],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> None:
    args = parse_args()
    canonical_rows = read_csv_rows(args.canonical_matrix_csv)
    critical_rows = read_csv_rows(args.critical_surface_csv)
    exception_rows = read_csv_rows(args.exception_register_csv)

    violations, layer_inventory, metrics = analyze(
        canonical_rows=canonical_rows,
        critical_rows=critical_rows,
        exception_rows=exception_rows,
    )

    gate_pass = len(violations) == 0 or args.policy_mode == "warn"
    write_summary(
        path=args.summary_csv,
        args=args,
        gate_pass=gate_pass,
        violations=violations,
        layer_inventory=layer_inventory,
        metrics=metrics,
    )
    if args.status_json:
        write_status_json(
            path=args.status_json,
            args=args,
            gate_pass=gate_pass,
            violations=violations,
            layer_inventory=layer_inventory,
            metrics=metrics,
        )

    if len(violations) > 0 and args.policy_mode == "fail":
        preview = ", ".join(f"{v.type}:{v.surface_key}" for v in violations[:8])
        raise SystemExit(
            "mock/exception gate failed: unresolved policy violations -> "
            f"{preview}"
        )


if __name__ == "__main__":
    main()
