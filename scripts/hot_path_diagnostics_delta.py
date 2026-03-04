#!/usr/bin/env python3
"""Compute reproducible before/after hot-path diagnostics deltas from artifact roots.

This script is intentionally tolerant of legacy artifacts that predate newer
`hot_path_*`/`diagnostics_*` stdout breadcrumbs. When direct counters are not
present, it falls back to deterministic manifest-derived proxies and records
the provenance in output rows.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build before/after delta rows for hot-path diagnostics "
            "(transport, scratch writes, pump cadence/forced decisions)."
        )
    )
    parser.add_argument("--baseline-root", required=True, type=Path)
    parser.add_argument("--post-root", required=True, type=Path)
    parser.add_argument("--baseline-label", default="baseline")
    parser.add_argument("--post-label", default="post_opt")
    parser.add_argument("--out-csv", required=True, type=Path)
    parser.add_argument("--out-json", type=Path)
    return parser.parse_args()


def read_text(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8")


def read_manifest(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Invalid JSON in manifest {path}: {exc}") from exc


def as_int(value: Any) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return int(value)
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def as_bool(value: Any) -> bool | None:
    if isinstance(value, bool):
        return value
    if value in ("true", "True", "1"):
        return True
    if value in ("false", "False", "0"):
        return False
    return None


def extract_int(pattern: str, text: str) -> int | None:
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        return None
    return as_int(match.group(1))


@dataclass
class Metric:
    value: int | bool | None
    source: str


@dataclass
class Snapshot:
    label: str
    root: Path
    metrics: dict[str, Metric]


def extract_snapshot(label: str, root: Path) -> Snapshot:
    manifest = read_manifest(root / "runtime.manifest.json")
    stdout = read_text(root / "runtime.stdout.log")

    asr_worker_pool = manifest.get("asr_worker_pool") or {}
    chunk_queue = manifest.get("chunk_queue") or {}
    cleanup_queue = manifest.get("cleanup_queue") or {}

    submitted = as_int(asr_worker_pool.get("submitted"))
    temp_audio_deleted = as_int(asr_worker_pool.get("temp_audio_deleted"))
    temp_audio_retained = as_int(asr_worker_pool.get("temp_audio_retained"))

    metrics: dict[str, Metric] = {
        "submitted": Metric(submitted, "manifest.asr_worker_pool.submitted"),
        "temp_audio_deleted": Metric(
            temp_audio_deleted, "manifest.asr_worker_pool.temp_audio_deleted"
        ),
        "temp_audio_retained": Metric(
            temp_audio_retained, "manifest.asr_worker_pool.temp_audio_retained"
        ),
        "chunk_queue_drain_completed": Metric(
            as_bool(chunk_queue.get("drain_completed")),
            "manifest.chunk_queue.drain_completed",
        ),
        "cleanup_queue_drain_completed": Metric(
            as_bool(cleanup_queue.get("drain_completed")),
            "manifest.cleanup_queue.drain_completed",
        ),
        "lifecycle_transition_count": Metric(
            extract_int(r"transition_count=(\d+)", stdout),
            (
                "stdout.lifecycle.transition_count"
                if "transition_count=" in stdout
                else "missing"
            ),
        ),
        "progressive_out_wav_materializations": Metric(
            extract_int(r"progressive_out_wav_materializations:\s*(\d+)", stdout),
            (
                "stdout.progressive_out_wav_materializations"
                if "progressive_out_wav_materializations:" in stdout
                else "missing"
            ),
        ),
    }

    # Transport distribution.
    hot_path_transport_match = re.search(
        r"hot_path_transport:\s+request_input_path=(\d+)\s+request_input_pcm_window=(\d+)",
        stdout,
        re.MULTILINE,
    )
    diagnostics_transport_match = re.search(
        r"diagnostics_transport=path:(\d+)\s+pcm_window:(\d+)",
        stdout,
        re.MULTILINE,
    )

    if hot_path_transport_match:
        metrics["request_input_path"] = Metric(
            as_int(hot_path_transport_match.group(1)), "stdout.hot_path_transport"
        )
        metrics["request_input_pcm_window"] = Metric(
            as_int(hot_path_transport_match.group(2)), "stdout.hot_path_transport"
        )
    elif diagnostics_transport_match:
        metrics["request_input_path"] = Metric(
            as_int(diagnostics_transport_match.group(1)), "stdout.diagnostics_transport"
        )
        metrics["request_input_pcm_window"] = Metric(
            as_int(diagnostics_transport_match.group(2)), "stdout.diagnostics_transport"
        )
    else:
        # Legacy fallback: each temp-audio artifact implies one path-mode request.
        path_count = None
        if temp_audio_deleted is not None and temp_audio_retained is not None:
            path_count = temp_audio_deleted + temp_audio_retained
        pcm_count = None
        if submitted is not None and path_count is not None:
            pcm_count = max(submitted - path_count, 0)
        metrics["request_input_path"] = Metric(
            path_count,
            "fallback.manifest.temp_audio_deleted_plus_retained",
        )
        metrics["request_input_pcm_window"] = Metric(
            pcm_count,
            "fallback.manifest.submitted_minus_path_count",
        )

    # Scratch write counters.
    hot_path_scratch_match = re.search(
        r"hot_path_scratch:\s+worker_paths_max=(\d+)\s+writes_est=(\d+)\s+reuse_overwrites_est=(\d+)",
        stdout,
        re.MULTILINE,
    )
    diagnostics_scratch_match = re.search(
        r"diagnostics_scratch=worker_paths_max:(\d+)\s+writes_est:(\d+)\s+reuse_overwrites_est:(\d+)",
        stdout,
        re.MULTILINE,
    )

    if hot_path_scratch_match:
        metrics["scratch_worker_paths_max"] = Metric(
            as_int(hot_path_scratch_match.group(1)), "stdout.hot_path_scratch"
        )
        metrics["scratch_writes_est"] = Metric(
            as_int(hot_path_scratch_match.group(2)), "stdout.hot_path_scratch"
        )
        metrics["scratch_reuse_overwrites_est"] = Metric(
            as_int(hot_path_scratch_match.group(3)), "stdout.hot_path_scratch"
        )
    elif diagnostics_scratch_match:
        metrics["scratch_worker_paths_max"] = Metric(
            as_int(diagnostics_scratch_match.group(1)), "stdout.diagnostics_scratch"
        )
        metrics["scratch_writes_est"] = Metric(
            as_int(diagnostics_scratch_match.group(2)), "stdout.diagnostics_scratch"
        )
        metrics["scratch_reuse_overwrites_est"] = Metric(
            as_int(diagnostics_scratch_match.group(3)), "stdout.diagnostics_scratch"
        )
    else:
        # Legacy fallback: path-mode temp artifacts are the closest available
        # scratch-write proxy in older manifests.
        proxy_writes = None
        if temp_audio_deleted is not None and temp_audio_retained is not None:
            proxy_writes = temp_audio_deleted + temp_audio_retained
        metrics["scratch_worker_paths_max"] = Metric(None, "missing")
        metrics["scratch_writes_est"] = Metric(
            proxy_writes, "fallback.manifest.temp_audio_deleted_plus_retained"
        )
        metrics["scratch_reuse_overwrites_est"] = Metric(None, "missing")

    # Pump counters.
    hot_path_pump_match = re.search(
        r"hot_path_pump:\s+chunk_decisions=(\d+)\s+forced_decisions=(\d+)\s+forced_capture_event_triggers=(\d+)\s+forced_shutdown_triggers=(\d+)",
        stdout,
        re.MULTILINE,
    )
    diagnostics_pump_match = re.search(
        r"diagnostics_pump=chunk_decisions:(\d+)\s+forced_decisions:(\d+)\s+forced_capture_event_triggers:(\d+)\s+forced_shutdown_triggers:(\d+)",
        stdout,
        re.MULTILINE,
    )

    if hot_path_pump_match:
        metrics["pump_chunk_decisions"] = Metric(
            as_int(hot_path_pump_match.group(1)), "stdout.hot_path_pump"
        )
        metrics["pump_forced_decisions"] = Metric(
            as_int(hot_path_pump_match.group(2)), "stdout.hot_path_pump"
        )
        metrics["pump_forced_capture_event_triggers"] = Metric(
            as_int(hot_path_pump_match.group(3)), "stdout.hot_path_pump"
        )
        metrics["pump_forced_shutdown_triggers"] = Metric(
            as_int(hot_path_pump_match.group(4)), "stdout.hot_path_pump"
        )
    elif diagnostics_pump_match:
        metrics["pump_chunk_decisions"] = Metric(
            as_int(diagnostics_pump_match.group(1)), "stdout.diagnostics_pump"
        )
        metrics["pump_forced_decisions"] = Metric(
            as_int(diagnostics_pump_match.group(2)), "stdout.diagnostics_pump"
        )
        metrics["pump_forced_capture_event_triggers"] = Metric(
            as_int(diagnostics_pump_match.group(3)), "stdout.diagnostics_pump"
        )
        metrics["pump_forced_shutdown_triggers"] = Metric(
            as_int(diagnostics_pump_match.group(4)), "stdout.diagnostics_pump"
        )
    else:
        metrics["pump_chunk_decisions"] = Metric(None, "missing")
        metrics["pump_forced_decisions"] = Metric(None, "missing")
        metrics["pump_forced_capture_event_triggers"] = Metric(None, "missing")
        metrics["pump_forced_shutdown_triggers"] = Metric(None, "missing")

    return Snapshot(label=label, root=root, metrics=metrics)


def delta_numeric(a: int | None, b: int | None) -> int | None:
    if a is None or b is None:
        return None
    return b - a


def encode(value: int | bool | None) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def build_rows(baseline: Snapshot, post: Snapshot) -> list[dict[str, str]]:
    metric_specs = [
        (
            "scratch_writes_est",
            "count",
            "scratch/temp write volume estimate",
            "fallback proxy in legacy artifacts equals temp_audio_deleted+temp_audio_retained",
        ),
        (
            "scratch_reuse_overwrites_est",
            "count",
            "scratch reuse overwrite estimate",
            "only present in newer hot_path diagnostics",
        ),
        (
            "request_input_path",
            "count",
            "request mode distribution: path",
            "fallback proxy in legacy artifacts uses temp_audio counters",
        ),
        (
            "request_input_pcm_window",
            "count",
            "request mode distribution: pcm_window",
            "fallback proxy in legacy artifacts uses submitted-path_count",
        ),
        (
            "pump_chunk_decisions",
            "count",
            "pump cadence: chunk decisions",
            "legacy artifacts may not include explicit pump breadcrumbs",
        ),
        (
            "pump_forced_decisions",
            "count",
            "pump cadence: forced decisions",
            "legacy artifacts may not include explicit pump breadcrumbs",
        ),
        (
            "pump_forced_capture_event_triggers",
            "count",
            "pump forced capture-event triggers",
            "legacy artifacts may not include explicit pump breadcrumbs",
        ),
        (
            "pump_forced_shutdown_triggers",
            "count",
            "pump forced shutdown triggers",
            "legacy artifacts may not include explicit pump breadcrumbs",
        ),
        (
            "chunk_queue_drain_completed",
            "bool",
            "chunk queue drain completion",
            "manifest-level drain completion flag",
        ),
    ]

    rows: list[dict[str, str]] = []
    for metric_id, unit, desc, notes in metric_specs:
        b_metric = baseline.metrics[metric_id]
        p_metric = post.metrics[metric_id]
        b_value = b_metric.value
        p_value = p_metric.value
        delta = (
            delta_numeric(as_int(b_value), as_int(p_value))
            if unit == "count"
            else None
        )
        rows.append(
            {
                "metric_id": metric_id,
                "description": desc,
                "unit": unit,
                f"{baseline.label}_value": encode(b_value),
                f"{post.label}_value": encode(p_value),
                "delta_post_minus_baseline": encode(delta),
                f"{baseline.label}_source": b_metric.source,
                f"{post.label}_source": p_metric.source,
                "notes": notes,
            }
        )
    return rows


def write_csv(path: Path, rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        raise ValueError("No rows generated")
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    baseline = extract_snapshot(args.baseline_label, args.baseline_root)
    post = extract_snapshot(args.post_label, args.post_root)
    rows = build_rows(baseline, post)
    write_csv(args.out_csv, rows)

    if args.out_json:
        payload = {
            "baseline_label": baseline.label,
            "baseline_root": str(baseline.root),
            "post_label": post.label,
            "post_root": str(post.root),
            "rows": rows,
            "raw_metrics": {
                baseline.label: {
                    key: {"value": encode(metric.value), "source": metric.source}
                    for key, metric in baseline.metrics.items()
                },
                post.label: {
                    key: {"value": encode(metric.value), "source": metric.source}
                    for key, metric in post.metrics.items()
                },
            },
        }
        write_json(args.out_json, payload)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
