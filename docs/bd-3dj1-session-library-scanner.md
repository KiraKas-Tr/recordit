# bd-3dj1 Session Library Scanner

Implemented `FileSystemSessionLibraryService` (`app/Services/FileSystemSessionLibraryService.swift`) as the canonical root-backed scanner/indexer for session history.

## Root Policy

- Uses app-managed canonical sessions root:
  - `~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/sessions/`
- Supports `RECORDIT_CONTAINER_DATA_ROOT` override for deterministic testing and tooling.

## Index Inputs

- Valid completed/degraded/failed session: directory containing `session.manifest.json`.
- Valid pending record-only session: directory containing both:
  - `session.pending.json`
  - `session.wav`

## Indexed Metadata

For each discovered session root the service emits:

- `sessionID`
- `mode`
- `status`
- `durationMs`
- `startedAt`
- `rootPath`

Metadata source precedence:

1. `session.manifest.json` (`runtime_mode`, `session_summary.session_status`, `session_summary.duration_sec`, `generated_at_utc`)
2. `session.pending.json` (`session_id`, `created_at_utc`, `mode`)
3. Path-derived fallback (`<timestamp>-<mode>` directory naming)

## Deterministic Ordering and Filtering

- Deterministic newest-first sort:
  1. `startedAt` descending
  2. `sessionID` ascending
  3. `rootPath` ascending
- Supports `SessionQuery` filters:
  - `status`
  - `mode`
  - case-insensitive `searchText`

## Degradation Behavior

- Missing sessions root returns an empty list (non-fatal).
- Missing/invalid optional artifacts degrade gracefully; session remains indexable when minimal validity conditions are met.
