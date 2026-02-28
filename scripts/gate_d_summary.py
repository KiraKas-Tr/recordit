#!/usr/bin/env python3
"""Summarize Gate D soak runs into a key/value CSV artifact."""

from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs-csv", required=True, type=Path)
    parser.add_argument("--summary-csv", required=True, type=Path)
    parser.add_argument("--target-seconds", required=True, type=int)
    return parser.parse_args()


def as_float(value: str) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0


def as_int(value: str) -> int:
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return 0


def as_bool(value: str) -> bool:
    return str(value).strip().lower() in {"1", "true", "yes", "y", "on"}


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * pct
    lo = int(position)
    hi = min(lo + 1, len(ordered) - 1)
    if lo == hi:
        return ordered[lo]
    return ordered[lo] * (hi - position) + ordered[hi] * (position - lo)


def parse_utc(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def main() -> None:
    args = parse_args()
    rows: list[dict[str, str]] = []
    with args.runs_csv.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            rows.append(row)
    if not rows:
        raise SystemExit(f"no rows found in {args.runs_csv}")

    real_ms = [as_float(r["real_ms"]) for r in rows]
    rss_kb = [as_float(r["max_rss_kb"]) for r in rows]
    wall_ms_p95 = [as_float(r["manifest_wall_ms_p95"]) for r in rows]
    lag_p95_ms = [as_float(r["chunk_lag_p95_ms"]) for r in rows]

    run_count = len(rows)
    failure_count = sum(1 for r in rows if as_int(r["exit_code"]) != 0)
    success_count = run_count - failure_count
    near_live_mode_count = sum(
        1 for r in rows if r["runtime_mode"] in {"live-chunked", "near-live"}
    )
    live_chunked_count = sum(1 for r in rows if as_bool(r["live_chunked"]))
    out_wav_materialized_count = sum(
        1 for r in rows if as_bool(r["out_wav_materialized"])
    )
    chunk_queue_visible_count = sum(
        1
        for r in rows
        if as_bool(r["chunk_queue_enabled"]) and as_int(r["chunk_submitted"]) > 0
    )
    chunk_drain_count = sum(
        1
        for r in rows
        if as_bool(r["chunk_drain_completed"]) and as_int(r["chunk_pending"]) == 0
    )
    capture_telemetry_readable_count = sum(
        1 for r in rows if as_bool(r["capture_telemetry_readable"])
    )
    reconciliation_applied_runs = sum(
        1 for r in rows if as_bool(r["reconciliation_applied"])
    )

    total_chunk_submitted = sum(as_int(r["chunk_submitted"]) for r in rows)
    total_chunk_dropped_oldest = sum(as_int(r["chunk_dropped_oldest"]) for r in rows)
    total_chunk_high_water = sum(as_int(r["chunk_high_water"]) for r in rows)
    max_chunk_high_water = max((as_int(r["chunk_high_water"]) for r in rows), default=0)
    max_chunk_queue_cap = max((as_int(r["chunk_max_queue"]) for r in rows), default=0)
    total_trust_notices = sum(as_int(r["trust_notice_count"]) for r in rows)
    total_degradation_events = sum(as_int(r["degradation_event_count"]) for r in rows)
    total_capture_restarts = sum(max(0, as_int(r["capture_restart_count"])) for r in rows)
    drop_ratio = (
        float(total_chunk_dropped_oldest) / float(total_chunk_submitted)
        if total_chunk_submitted > 0
        else 0.0
    )
    total_cleanup_dropped = sum(as_int(r["cleanup_dropped_queue_full"]) for r in rows)
    total_cleanup_failed = sum(as_int(r["cleanup_failed"]) for r in rows)
    total_cleanup_timed_out = sum(as_int(r["cleanup_timed_out"]) for r in rows)

    soak_start = parse_utc(rows[0]["start_utc"])
    soak_end = parse_utc(rows[-1]["end_utc"])
    # CSV timestamps are second-granularity and both endpoints are inclusive.
    soak_seconds_actual = max(0, int((soak_end - soak_start).total_seconds()) + 1)

    max_rss_kb_p50 = percentile(rss_kb, 0.50)
    max_rss_kb_p95 = percentile(rss_kb, 0.95)
    manifest_wall_ms_p95_p50 = percentile(wall_ms_p95, 0.50)
    manifest_wall_ms_p95_p95 = percentile(wall_ms_p95, 0.95)
    chunk_lag_p95_ms_p50 = percentile(lag_p95_ms, 0.50)
    chunk_lag_p95_ms_p95 = percentile(lag_p95_ms, 0.95)

    threshold_soak_duration_ok = soak_seconds_actual >= args.target_seconds
    threshold_harness_reliability_ok = failure_count == 0
    threshold_latency_drift_ok = (
        manifest_wall_ms_p95_p50 > 0
        and manifest_wall_ms_p95_p95 <= 1.25 * manifest_wall_ms_p95_p50
    )
    threshold_memory_growth_ok = (
        max_rss_kb_p50 > 0 and max_rss_kb_p95 <= 1.30 * max_rss_kb_p50
    )
    threshold_near_live_mode_ok = (
        near_live_mode_count == run_count and live_chunked_count == run_count
    )
    threshold_chunk_queue_visibility_ok = chunk_queue_visible_count == run_count
    threshold_chunk_drain_ok = chunk_drain_count == run_count
    threshold_out_wav_truth_ok = out_wav_materialized_count == run_count
    threshold_continuity_signal_ok = capture_telemetry_readable_count == run_count
    threshold_lag_drift_ok = (
        chunk_lag_p95_ms_p50 > 0
        and chunk_lag_p95_ms_p95 <= 1.50 * chunk_lag_p95_ms_p50
    )

    gate_pass = all(
        [
            threshold_soak_duration_ok,
            threshold_harness_reliability_ok,
            threshold_latency_drift_ok,
            threshold_memory_growth_ok,
            threshold_near_live_mode_ok,
            threshold_chunk_queue_visibility_ok,
            threshold_chunk_drain_ok,
            threshold_out_wav_truth_ok,
            threshold_continuity_signal_ok,
            threshold_lag_drift_ok,
        ]
    )

    args.summary_csv.parent.mkdir(parents=True, exist_ok=True)
    with args.summary_csv.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(["key", "value"])
        writer.writerow(
            [
                "generated_at_utc",
                datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            ]
        )
        writer.writerow(["artifact_track", "gate_d_near_live"])
        writer.writerow(["run_count", run_count])
        writer.writerow(["success_count", success_count])
        writer.writerow(["failure_count", failure_count])
        writer.writerow(["soak_seconds_target", args.target_seconds])
        writer.writerow(["soak_seconds_actual", soak_seconds_actual])
        writer.writerow(["real_ms_p50", percentile(real_ms, 0.50)])
        writer.writerow(["real_ms_p95", percentile(real_ms, 0.95)])
        writer.writerow(["max_rss_kb_p50", max_rss_kb_p50])
        writer.writerow(["max_rss_kb_p95", max_rss_kb_p95])
        writer.writerow(["manifest_wall_ms_p95_p50", manifest_wall_ms_p95_p50])
        writer.writerow(["manifest_wall_ms_p95_p95", manifest_wall_ms_p95_p95])
        writer.writerow(["chunk_lag_p95_ms_p50", chunk_lag_p95_ms_p50])
        writer.writerow(["chunk_lag_p95_ms_p95", chunk_lag_p95_ms_p95])
        writer.writerow(["near_live_mode_count", near_live_mode_count])
        writer.writerow(["live_chunked_count", live_chunked_count])
        writer.writerow(["out_wav_materialized_count", out_wav_materialized_count])
        writer.writerow(["chunk_queue_visible_count", chunk_queue_visible_count])
        writer.writerow(["chunk_drain_complete_count", chunk_drain_count])
        writer.writerow(["capture_telemetry_readable_count", capture_telemetry_readable_count])
        writer.writerow(["reconciliation_applied_runs", reconciliation_applied_runs])
        writer.writerow(["total_chunk_submitted", total_chunk_submitted])
        writer.writerow(["total_chunk_dropped_oldest", total_chunk_dropped_oldest])
        writer.writerow(["total_chunk_high_water", total_chunk_high_water])
        writer.writerow(["max_chunk_high_water", max_chunk_high_water])
        writer.writerow(["max_chunk_queue_cap", max_chunk_queue_cap])
        writer.writerow(["chunk_drop_ratio", drop_ratio])
        writer.writerow(["total_trust_notices", total_trust_notices])
        writer.writerow(["total_cleanup_dropped", total_cleanup_dropped])
        writer.writerow(["total_cleanup_failed", total_cleanup_failed])
        writer.writerow(["total_cleanup_timed_out", total_cleanup_timed_out])
        writer.writerow(["total_degradation_events", total_degradation_events])
        writer.writerow(["total_capture_restarts", total_capture_restarts])
        writer.writerow(
            ["threshold_soak_duration_ok", str(threshold_soak_duration_ok).lower()]
        )
        writer.writerow(
            [
                "threshold_harness_reliability_ok",
                str(threshold_harness_reliability_ok).lower(),
            ]
        )
        writer.writerow(
            ["threshold_latency_drift_ok", str(threshold_latency_drift_ok).lower()]
        )
        writer.writerow(
            ["threshold_memory_growth_ok", str(threshold_memory_growth_ok).lower()]
        )
        writer.writerow(
            ["threshold_near_live_mode_ok", str(threshold_near_live_mode_ok).lower()]
        )
        writer.writerow(
            [
                "threshold_chunk_queue_visibility_ok",
                str(threshold_chunk_queue_visibility_ok).lower(),
            ]
        )
        writer.writerow(
            ["threshold_chunk_drain_ok", str(threshold_chunk_drain_ok).lower()]
        )
        writer.writerow(
            ["threshold_out_wav_truth_ok", str(threshold_out_wav_truth_ok).lower()]
        )
        writer.writerow(
            [
                "threshold_continuity_signal_ok",
                str(threshold_continuity_signal_ok).lower(),
            ]
        )
        writer.writerow(["threshold_lag_drift_ok", str(threshold_lag_drift_ok).lower()])
        writer.writerow(["gate_pass", str(gate_pass).lower()])

    print(args.summary_csv)


if __name__ == "__main__":
    main()
