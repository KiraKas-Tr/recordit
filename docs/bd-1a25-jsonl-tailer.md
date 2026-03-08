# bd-1a25: Robust JSONL Tailer with Cursor Persistence

Date: 2026-03-05

## Scope

Implemented a file-system JSONL tailer service for runtime transcript streaming:

1. `app/Services/FileSystemJsonlTailService.swift`
2. `app/Services/jsonl_tailer_smoke.swift`

## Behavior

### Complete-line parsing only

The tailer consumes only newline-terminated rows (`\n`).

- trailing bytes without a newline are deferred
- cursor byte offset is not advanced past deferred partial bytes

### Malformed-line tolerance

Each complete line is parsed independently:

- malformed JSON or missing `event_type` lines are skipped
- parsing continues for subsequent lines
- malformed line count is tracked in diagnostics

### Cursor persistence semantics

`JsonlTailCursor` persists:

1. `byteOffset`
2. `lineCount`
3. `lastModifiedAt`

Resume behavior:

- reading from persisted cursor avoids replaying already-consumed stable rows
- truncation/rewrite guard resets to start when cursor is invalid for current file size or mtime regresses

## Mapping to DTO

Each parsed row maps into `RuntimeEventDTO` with:

1. required: `event_type`
2. optional: `channel`, `segment_id`, `start_ms`, `end_ms`, `text`
3. additional unknown fields captured in `payload` for compatibility/debugging

## Validation

`jsonl_tailer_smoke.swift` verifies:

1. complete-line-only parsing with partial-line deferral
2. malformed-line skip without stream halt
3. completed partial line emitted on subsequent append
4. persisted cursor resume with no duplicate stable rows
