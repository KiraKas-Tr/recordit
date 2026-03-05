# bd-1tuy: Diagnostics Redaction Pipeline and Opt-In Gates

## Goal

Ensure diagnostics exports redact transcript text by default across manifest/JSONL payloads, and only include transcript text when explicit opt-in is requested.

## Delivered

1. `app/Exports/export_smoke.swift`
2. `docs/bd-1tuy-diagnostics-redaction.md`

## What Landed

1. Extended diagnostics export smoke coverage for default-redacted behavior:
- verifies diagnostics manifest contains redacted transcript placeholders (`[REDACTED]`)
- verifies diagnostics JSONL scrubs transcript text fields (`"text":"[REDACTED]"`)
- verifies default diagnostics metadata records `include_transcript_text = false`

2. Added explicit opt-in diagnostics assertions:
- verifies opt-in export reports `redacted == false`
- verifies manifest transcript content is preserved when opt-in is enabled
- verifies JSONL transcript text is preserved when opt-in is enabled
- verifies diagnostics metadata records `include_transcript_text = true`

3. Kept policy guard coverage intact for managed-storage export destination enforcement.

## Acceptance Mapping

1. Diagnostics transcript text redacted by default:
- validated through manifest + JSONL assertions in smoke.

2. Explicit opt-in includes transcript text:
- validated through opt-in diagnostics assertions in smoke.

3. Redaction behavior covered by test gate:
- `export_smoke` now fails if default/opt-in transcript privacy semantics regress.
