# bd-2yrp Legacy Flat Artifact Migration Ingest

## Scope

Implemented migration ingest for legacy flat artifact layout:
- Detect flat artifact sets under sessions root using `<session-stem>.*` naming.
- Ingest legacy sets as import-only session records.
- Avoid duplicate imports when canonical session directories already represent the same session stem or ID.
- Surface migration details in diagnostics and emit scan outcome logs.

## Implementation

### `app/Services/ServiceInterfaces.swift`
- Added `SessionIngestSource` enum:
  - `canonical_directory`
  - `legacy_flat_import`
- Extended `SessionSummaryDTO` with:
  - `ingestSource`
  - `ingestDiagnostics`

### `app/Services/FileSystemSessionLibraryService.swift`
- Added legacy flat artifact discovery at sessions root:
  - `*.manifest.json`
  - `*.pending.json`
  - `*.wav`
  - `*.input.wav`
  - `*.jsonl`
- Added deterministic grouping by stem into legacy artifact sets.
- Added import-only indexing for valid sets (audio + at least one metadata/transcript file).
- Added dedupe keys (session ID + stem) to skip duplicate imports against canonical discovered sessions.
- Added migration diagnostics payload on imported summaries:
  - ingest source
  - legacy stem
  - source artifact paths
- Added stderr logging with discovery/import/duplicate-skip counts.
- Ensured legacy ingest does not rewrite source artifacts.

### `app/Services/legacy_flat_migration_smoke.swift`
- Added focused smoke coverage for:
  - canonical + legacy mixed discovery
  - duplicate skip when canonical stem already exists
  - deterministic legacy diagnostics fields
  - no source artifact rewrites across scans

## Validation

Run:

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/SessionTranscriptSearchIndex.swift \
  app/Services/FileSystemSessionLibraryService.swift \
  app/Services/MockServices.swift \
  -module-name RecordItLegacyFlatMigration \
  -o /tmp/RecordItLegacyFlatMigration.swiftmodule

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/PendingSessionTransitionService.swift \
  app/Services/PendingSessionSidecarService.swift \
  app/Services/SessionTranscriptSearchIndex.swift \
  app/Services/FileSystemSessionLibraryService.swift \
  app/Services/legacy_flat_migration_smoke.swift \
  -o /tmp/legacy_flat_migration_smoke && /tmp/legacy_flat_migration_smoke

UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/Services/ServiceInterfaces.swift \
  app/Services/FileSystemSessionLibraryService.swift \
  app/Services/legacy_flat_migration_smoke.swift \
  docs/bd-2yrp-legacy-migration-ingest.md
```
