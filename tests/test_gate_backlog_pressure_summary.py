#!/usr/bin/env python3
"""Regression checks for gate_backlog_pressure_summary JSONL parsing."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SCRIPTS_DIR = PROJECT_ROOT / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from gate_backlog_pressure_summary import parse_jsonl  # noqa: E402


class GateBacklogPressureSummaryTests(unittest.TestCase):
    def _write_temp_jsonl(self, contents: str) -> Path:
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as handle:
            handle.write(contents)
            return Path(handle.name)

    def test_parse_jsonl_accepts_object_lines(self) -> None:
        jsonl_path = self._write_temp_jsonl(
            '{"event_type":"chunk_queue","queue_depth":7}\n'
            '{"event_type":"partial","seq":1}\n'
        )
        self.addCleanup(lambda: jsonl_path.unlink(missing_ok=True))

        parsed = parse_jsonl(jsonl_path)
        self.assertEqual(len(parsed), 2)
        self.assertEqual(parsed[0]["event_type"], "chunk_queue")
        self.assertEqual(parsed[1]["event_type"], "partial")

    def test_parse_jsonl_rejects_malformed_line_with_context(self) -> None:
        jsonl_path = self._write_temp_jsonl('{"event_type":"chunk_queue"}\nnot-json\n')
        self.addCleanup(lambda: jsonl_path.unlink(missing_ok=True))

        with self.assertRaisesRegex(
            ValueError, rf"invalid JSONL line at {jsonl_path}:2:"
        ):
            parse_jsonl(jsonl_path)

    def test_parse_jsonl_rejects_non_object_line(self) -> None:
        jsonl_path = self._write_temp_jsonl('{"event_type":"chunk_queue"}\n[]\n')
        self.addCleanup(lambda: jsonl_path.unlink(missing_ok=True))

        with self.assertRaisesRegex(
            ValueError, rf"invalid JSONL line at {jsonl_path}:2: expected object"
        ):
            parse_jsonl(jsonl_path)


if __name__ == "__main__":
    unittest.main()
