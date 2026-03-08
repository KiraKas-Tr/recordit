# bd-2snv: Diagnostics Payload Schema and Support Minimums

## Goal

Define a stable diagnostics payload schema that gives support enough context for triage while keeping transcript privacy defaults explicit.

## Delivered

1. `app/Exports/SessionExportService.swift`
2. `app/Exports/export_smoke.swift`
3. `docs/bd-2snv-diagnostics-schema.md`

## Schema Additions (`diagnostics.json`)

Top-level fields now include:
1. `schema_version`
2. `kind`
3. `generated_at_utc`
4. `session_id`
5. `include_transcript_text`
6. `include_audio`
7. `artifacts`
8. `redaction_contract`
9. `support_snapshot`

### `redaction_contract`

1. `mode` (`redact_default` | `include_opt_in`)
2. `transcript_text_included` (bool)
3. `redacted_text_keys` (array)

### `support_snapshot`

1. `schema_version`
2. `manifest_summary`
3. `counters`

#### `manifest_summary` minimums

1. `manifest_valid`
2. `runtime_mode`
3. `session_status`
4. `duration_sec`
5. `trust_notice_count`
6. `degradation_codes`
7. `failure_context` (`code`, `message`)

#### `counters` minimums

1. `jsonl_present`
2. `line_count`
3. `unparseable_line_count`
4. `event_type_counts`

## Acceptance Mapping

1. Required support fields with version marker: covered by `support_snapshot.schema_version` and minimum keys.
2. Redaction contract explicit: covered by `redaction_contract` semantics.
3. Sample payload validates in tests/tools: enforced via `export_smoke` JSON assertions for both default-redacted and opt-in modes.

## Validation

`export_smoke` now validates:
1. default-redacted contract + support snapshot structure
2. presence of trust/degradation/failure-context/counter minimums
3. opt-in contract mode and transcript inclusion behavior
