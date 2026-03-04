#!/usr/bin/env python3
"""Regression checks for schema-tolerant manifest signal extraction."""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SCRIPTS_DIR = PROJECT_ROOT / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from manifest_signal_extract import extract_manifest_signal_codes  # noqa: E402


class ManifestSignalExtractTests(unittest.TestCase):
    def test_representative_chunked_extracts_expected_signals(self) -> None:
        manifest_path = (
            PROJECT_ROOT
            / "artifacts/validation/bd-1qfx/representative-chunked.runtime.manifest.json"
        )
        with manifest_path.open(encoding="utf-8") as handle:
            manifest = json.load(handle)

        extracted = extract_manifest_signal_codes(manifest)
        self.assertIn(
            extracted["trust_source"],
            {
                "trust.notices[].code",
                "trust.codes[]+trust.notices[].code",
                "session_summary.trust_notices.top_codes[]",
            },
        )
        self.assertEqual(extracted["degradation_source"], "degradation_events[].code")
        self.assertIn("chunk_queue_backpressure", extracted["trust_codes"])
        self.assertIn("chunk_queue_backpressure_severe", extracted["trust_codes"])
        self.assertIn(
            "live_chunk_queue_backpressure_severe", extracted["degradation_codes"]
        )

    def test_falls_back_to_session_summary_when_trust_is_null(self) -> None:
        manifest = {
            "trust": None,
            "degradation_events": [{"code": "live_chunk_queue_drop_oldest"}],
            "session_summary": {
                "trust_notices": {
                    "top_codes": [
                        "chunk_queue_backpressure",
                        "reconciliation_applied",
                        "chunk_queue_backpressure",
                    ]
                }
            },
        }
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(
            extracted["trust_source"], "session_summary.trust_notices.top_codes[]"
        )
        self.assertEqual(extracted["degradation_source"], "degradation_events[].code")
        self.assertEqual(
            extracted["trust_codes"],
            ["chunk_queue_backpressure", "reconciliation_applied"],
        )
        self.assertEqual(extracted["degradation_codes"], ["live_chunk_queue_drop_oldest"])

    def test_trust_codes_array_is_merged_with_notices_when_present(self) -> None:
        manifest = {
            "trust": {
                "codes": [
                    "capture_callback_contract_degraded",
                    "chunk_queue_backpressure",
                ],
                "notices": [
                    {"code": "chunk_queue_backpressure"},
                    {"code": "chunk_queue_backpressure_severe"},
                ],
            },
            "degradation_events": [{"code": "live_chunk_queue_backpressure_severe"}],
        }
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(
            extracted["trust_source"], "trust.codes[]+trust.notices[].code"
        )
        self.assertEqual(
            extracted["trust_codes"],
            [
                "capture_callback_contract_degraded",
                "chunk_queue_backpressure",
                "chunk_queue_backpressure_severe",
            ],
        )

    def test_trust_notices_are_used_when_trust_codes_array_is_missing(self) -> None:
        manifest = {
            "trust": {
                "notices": [
                    {"code": "chunk_queue_backpressure"},
                    {"code": "chunk_queue_backpressure_severe"},
                    {"code": "chunk_queue_backpressure"},
                ]
            }
        }
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(extracted["trust_source"], "trust.notices[].code")
        self.assertEqual(
            extracted["trust_codes"],
            ["chunk_queue_backpressure", "chunk_queue_backpressure_severe"],
        )

    def test_degradation_codes_fall_back_to_session_summary_when_events_missing(self) -> None:
        manifest = {
            "trust": None,
            "session_summary": {
                "trust_notices": {"top_codes": ["continuity_unverified"]},
                "degradation_events": {
                    "top_codes": [
                        "live_capture_transport_degraded",
                        "reconciliation_applied_after_backpressure",
                        "live_capture_transport_degraded",
                    ]
                },
            },
        }
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(
            extracted["degradation_source"],
            "session_summary.degradation_events.top_codes[]",
        )
        self.assertEqual(
            extracted["degradation_codes"],
            [
                "live_capture_transport_degraded",
                "reconciliation_applied_after_backpressure",
            ],
        )

    def test_degradation_source_is_none_when_no_codes_present(self) -> None:
        manifest = {"trust": None, "session_summary": {}}
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(extracted["degradation_source"], "none")
        self.assertEqual(extracted["degradation_codes"], [])

    def test_top_codes_dedup_preserves_order(self) -> None:
        manifest = {
            "trust": None,
            "session_summary": {
                "trust_notices": {
                    "top_codes": [
                        "zeta_signal",
                        "alpha_signal",
                        "zeta_signal",
                        "beta_signal",
                    ]
                }
            },
        }
        extracted = extract_manifest_signal_codes(manifest)
        self.assertEqual(
            extracted["trust_codes"],
            ["zeta_signal", "alpha_signal", "beta_signal"],
        )


if __name__ == "__main__":
    unittest.main()
