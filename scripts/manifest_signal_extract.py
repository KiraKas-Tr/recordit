#!/usr/bin/env python3
"""Extract trust/degradation signal codes from runtime manifests with schema fallbacks."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def _dedupe_preserving_order(items: list[str]) -> list[str]:
    seen: set[str] = set()
    deduped: list[str] = []
    for item in items:
        if item in seen:
            continue
        seen.add(item)
        deduped.append(item)
    return deduped


def _codes_from_dict_array(items: Any) -> list[str]:
    if not isinstance(items, list):
        return []
    codes = []
    for item in items:
        if not isinstance(item, dict):
            continue
        code = item.get("code")
        if isinstance(code, str) and code.strip():
            codes.append(code.strip())
    return _dedupe_preserving_order(codes)


def _codes_from_string_array(items: Any) -> list[str]:
    if not isinstance(items, list):
        return []
    codes = []
    for item in items:
        if isinstance(item, str) and item.strip():
            codes.append(item.strip())
    return _dedupe_preserving_order(codes)


def extract_manifest_signal_codes(manifest: dict[str, Any]) -> dict[str, Any]:
    trust_codes: list[str] = []
    trust_source = "none"

    trust = manifest.get("trust")
    if isinstance(trust, dict):
        trust_codes = _codes_from_string_array(trust.get("codes"))
        trust_notice_codes = _codes_from_dict_array(trust.get("notices"))
        if trust_codes and trust_notice_codes:
            trust_codes = _dedupe_preserving_order(trust_codes + trust_notice_codes)
            trust_source = "trust.codes[]+trust.notices[].code"
        elif trust_codes:
            trust_source = "trust.codes[]"
        elif trust_notice_codes:
            trust_codes = trust_notice_codes
            trust_source = "trust.notices[].code"

    if not trust_codes:
        session_summary = manifest.get("session_summary")
        if isinstance(session_summary, dict):
            trust_notices = session_summary.get("trust_notices")
            if isinstance(trust_notices, dict):
                trust_codes = _codes_from_string_array(trust_notices.get("top_codes"))
                if trust_codes:
                    trust_source = "session_summary.trust_notices.top_codes[]"

    degradation_codes = _codes_from_dict_array(manifest.get("degradation_events"))
    degradation_source = "none"
    if degradation_codes:
        degradation_source = "degradation_events[].code"
    else:
        session_summary = manifest.get("session_summary")
        if isinstance(session_summary, dict):
            degradation_summary = session_summary.get("degradation_events")
            if isinstance(degradation_summary, dict):
                degradation_codes = _codes_from_string_array(
                    degradation_summary.get("top_codes")
                )
                if degradation_codes:
                    degradation_source = "session_summary.degradation_events.top_codes[]"

    return {
        "trust_codes": trust_codes,
        "degradation_codes": degradation_codes,
        "trust_source": trust_source,
        "degradation_source": degradation_source,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    with args.manifest.open(encoding="utf-8") as handle:
        manifest = json.load(handle)
    if not isinstance(manifest, dict):
        raise SystemExit(f"manifest must be a JSON object: {args.manifest}")
    print(json.dumps(extract_manifest_signal_codes(manifest), sort_keys=True))


if __name__ == "__main__":
    main()
