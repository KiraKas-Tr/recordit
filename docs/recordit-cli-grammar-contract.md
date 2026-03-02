# Canonical `recordit` CLI Grammar Contract (bd-3ot7)

Date: 2026-03-02  
Status: canonical Phase C design contract for operator-facing CLI work

## 1. Design Goal

`recordit` is the human-first operator surface.

It must optimize for:
- one obvious happy path
- fewer output-path decisions
- explicit automation semantics
- additive coexistence with legacy `transcribe-live`

It must not expose the full engineering/control surface of `transcribe-live` by default.

## 2. Canonical Top-Level Grammar

```text
recordit run --mode <live|offline> [mode options] [shared options]
recordit doctor [doctor options]
recordit preflight [preflight options]
recordit replay --jsonl <path> [replay options]
recordit inspect-contract <cli|runtime-modes|jsonl-schema|manifest-schema|exit-codes> [--format json]
```

No additional top-level verbs are part of the canonical v1 operator surface.

## 3. Command Semantics

### 3.1 `recordit run`

Canonical operator entrypoint.

```text
recordit run --mode live [--duration-sec <seconds>] [--output-root <path>] [--profile <fast|balanced|quality>] [--language <tag>] [--model <path-or-id>] [--json]
recordit run --mode offline --input-wav <path> [--duration-sec <seconds>] [--output-root <path>] [--profile <fast|balanced|quality>] [--language <tag>] [--model <path-or-id>] [--json]
```

Rules:
- `--mode live` is the primary operator path.
- `--mode offline` is the deterministic non-live path.
- `--input-wav` is required for `--mode offline` and invalid for `--mode live`.
- if `--duration-sec` is omitted in live mode, the run is unbounded and continues until interrupted.
- if `--duration-sec` is provided, it must be greater than zero.
- `--output-root` is optional in both modes.
- `--json` adds a machine-readable terminal summary without changing artifact semantics.

### 3.2 `recordit doctor`

Environment/model/backend diagnostics without starting a full transcription run.

```text
recordit doctor [--model <path-or-id>] [--backend <auto|whispercpp|whisperkit|moonshine>] [--json]
```

Contract intent:
- surface configuration and dependency health clearly
- replace legacy `--model-doctor` as the human-facing discovery path

### 3.3 `recordit preflight`

Validate whether the next requested run is expected to succeed.

```text
recordit preflight [--mode <live|offline>] [--input-wav <path>] [--output-root <path>] [--json]
```

Contract intent:
- validate capture/model/output-path readiness
- emit the same core mode/output decisions `run` would use
- keep manifest semantics aligned with current `transcribe-live --preflight`

### 3.4 `recordit replay`

Replay previously emitted runtime JSONL for human review or automation.

```text
recordit replay --jsonl <path> [--format <text|json>]
```

Contract intent:
- make replay discoverable without teaching legacy flag vocabulary
- keep replay input contract tied to canonical runtime JSONL semantics

### 3.5 `recordit inspect-contract`

Machine-facing discovery surface for contract publication.

```text
recordit inspect-contract <cli|runtime-modes|jsonl-schema|manifest-schema|exit-codes> [--format json]
```

Contract intent:
- let agents/CI ask the binary for the canonical contract payloads
- avoid scraping help text or prose docs

## 4. Default Output-Root Contract

### 4.1 Core rule

Operators should choose **at most one** output location: `--output-root`.

They should never need to supply separate `--out-wav`, `--out-jsonl`, and `--out-manifest` paths on the canonical CLI.

### 4.2 Session-scoped default

If `--output-root` is omitted, `recordit run` materializes a session directory under the process-context artifact base:

```text
<artifact-base>/sessions/<YYYYMMDD>/<timestamp>-<mode>/
```

Examples:
- repo/debug context: `./artifacts/sessions/20260302/20260302T123456Z-live/`
- packaged app context: `<sandbox-artifact-root>/sessions/20260302/20260302T123456Z-live/`

### 4.3 Session artifact layout

Within one session root, canonical filenames are:
- `session.input.wav` (live mode when capture input is materialized)
- `session.wav`
- `session.jsonl`
- `session.manifest.json`

Optional/additive artifacts may appear beside them, but these names are the operator-facing quartet.

### 4.4 Explicit output-root behavior

If `--output-root <path>` is provided:
- `<path>` becomes the session root directly
- the canonical filenames above are created inside that directory
- the command must not silently fan out into unrelated sibling directories

## 5. Surface Area Split: Canonical vs Legacy

### 5.1 Surfaced on `recordit`

Keep these visible on the canonical CLI:
- mode selection: `live`, `offline`
- input selection for offline runs: `--input-wav`
- output location selection: `--output-root`
- human-meaningful quality/model selectors:
  - `--profile`
  - `--language`
  - `--model`
- diagnostics/replay/contract inspection:
  - `doctor`
  - `preflight`
  - `replay`
  - `inspect-contract`

### 5.2 Hidden in legacy `transcribe-live`

Keep these as expert-only/legacy controls unless later evidence proves they belong in the happy path:
- VAD backend and threshold tuning
- chunk window/stride/queue sizing
- live ASR worker count
- cleanup queue/retry/time budget controls
- explicit per-artifact output file flags
- benchmark-run count and deep engineering diagnostics
- low-level channel/speaker-label compatibility knobs that mainly exist for regression/gate work

## 6. Compatibility Positioning

`recordit` is additive in v1.

Compatibility commitments:
- `transcribe-live` remains available for existing scripts, gates, and expert workflows
- `recordit run --mode live` maps to the current true live-stream runtime intent
- `recordit run --mode offline` maps to the deterministic representative-offline intent
- legacy flags are not removed just because a smaller operator grammar now exists

## 7. Canonical Examples

```bash
recordit run --mode live
recordit run --mode offline --input-wav ./sample.wav
recordit doctor
recordit preflight --mode live
recordit replay --jsonl ./artifacts/sessions/20260302/20260302T123456Z-live/session.jsonl
recordit inspect-contract cli --format json
```

## 8. Acceptance Consequences for Follow-On Beads

This grammar fixes the design inputs for:
- `bd-d5zq` top-level dispatch and shared config plumbing
- `bd-24ks` offline mode implementation
- `bd-29pe` live mode implementation
- `bd-yn48` diagnostics/replay commands
- `bd-38gh` machine-readable CLI contract publication
- `bd-26wn` session-scoped output-root defaults and artifact layout

Any future expansion of the canonical grammar should be treated as additive-only unless this document is intentionally revised with migration rationale.
