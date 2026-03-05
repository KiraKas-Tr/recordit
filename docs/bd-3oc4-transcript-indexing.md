# bd-3oc4: Incremental Transcript Indexing for Session Search

Date: 2026-03-05

## Scope

Implemented incremental transcript indexing for session search in the Swift app service layer.

Files:

1. `app/Services/SessionTranscriptSearchIndex.swift`
2. `app/Services/FileSystemSessionLibraryService.swift`
3. `app/Services/session_search_index_smoke.swift`

## Design

### Incremental index cache

`SessionTranscriptSearchIndex` maintains an in-memory map:

- key: canonical `sessionRoot.path`
- value: transcript payload + artifact fingerprint

Fingerprint is based on `session.manifest.json` and `session.jsonl` signatures (size + mtime).

On each search call:

1. stale entries (sessions no longer present) are removed
2. unchanged fingerprints reuse cached transcript text
3. changed/new fingerprints trigger local re-extract + cache update

This keeps repeated searches fast while still reflecting new/updated session artifacts.

### Transcript precedence in index extraction

Index extraction follows the same precedence as session detail behavior:

1. `session.manifest.json` first
   - `terminal_summary.stable_lines` preferred
   - fallback to top-level `transcript`
2. `session.jsonl` fallback
   - `reconciled_final` preferred
   - then `llm_final`
   - then `final`

### Search integration

`FileSystemSessionLibraryService.listSessions(query:)` now applies search in two phases:

1. status/mode filtering first
2. text filtering as:
   - metadata haystack match (`sessionID`, folder name, timestamp) OR
   - transcript index match

This preserves deterministic sorting and filter semantics while adding transcript-aware search.

## Validation

Smoke runner `session_search_index_smoke.swift` validates:

1. manifest-backed transcript search hit
2. JSONL fallback search hit
3. status filter interactions with transcript search
4. incremental reindex behavior after JSONL update (old term disappears, new term appears)
