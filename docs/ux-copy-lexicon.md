# UX Copy Lexicon and String Catalog Rules (`bd-2m6f`)

Date: 2026-03-05  
Status: canonical English-first copy source for onboarding/runtime/export UX

## Purpose

Define one plain-language copy lexicon and one stable String Catalog key scheme so:

1. onboarding, runtime, and export views use the same language
2. trust/degraded/failure messaging stays technically correct
3. localization can be added without renaming keys

## Scope

This lexicon covers v1 user-facing copy for:

1. permissions and onboarding checks
2. model setup/readiness
3. runtime status and transcript lifecycle
4. degraded/failure notices and remediation prompts
5. export and diagnostics privacy prompts

Source scaffold file: `packaging/strings/Recordit.xcstrings`

## Voice Rules

1. Prefer direct verbs: `Grant access`, `Try again`, `Export`.
2. Prefer user terms over internals: `Needs permission` instead of `TCC denied`.
3. Keep sentence case, not title case, for body copy.
4. Keep each message to one action and one reason.
5. Avoid hidden blame language (`you failed`, `invalid user state`).
6. Keep technical detail in secondary text, not primary CTA labels.

## Status Vocabulary

Allowed session status labels:

1. `OK`: completed without trust notices.
2. `Degraded`: completed, but trust/degradation signals require review.
3. `Failed`: run did not complete cleanly.

Do not introduce alternative synonyms (`warning mode`, `partial fail`, `hard fail`) in UI copy.

## Key Naming Scheme

Key format:

`<surface>.<flow>.<topic>.<element>`

Examples:

1. `onboarding.permissions.screen_recording.title`
2. `runtime.status.degraded.body`
3. `export.diagnostics.include_transcript_text.label`

Rules:

1. Use lowercase snake_case segments.
2. Use nouns for topics and semantic element names for leaves (`title`, `body`, `action`).
3. Add keys only; do not rename existing keys.
4. If copy meaning changes, keep key and update value/comment unless meaning is materially different.
5. For materially different semantics, add a new key and deprecate old key in comment.

## Placeholder Rules

1. Use positional placeholders only when needed (`%@`, `%d`).
2. Keep placeholder semantics in comments.
3. Do not embed formatting markup in values.
4. Keep unit language explicit (`%d seconds`, not `%d` alone).

## View Reference Map

Use these key-prefix namespaces by UX surface:

1. Onboarding views: `onboarding.permissions.*`, `onboarding.model.*`, `onboarding.preflight.*`
2. Runtime views: `runtime.status.*`, `runtime.lifecycle.*`, `runtime.transcript.*`, `runtime.trust.*`
3. Export views: `export.transcript.*`, `export.bundle.*`, `export.diagnostics.*`, `export.privacy.*`

This map is the canonical reference contract for onboarding/runtime/export copy usage.

## Canonical Lexicon (English)

### Onboarding and Permissions

1. `Needs permission`
2. `Screen Recording access is required to capture system audio.`
3. `Microphone access is required to capture your voice.`
4. `Open System Settings`
5. `Re-check access`
6. `You may need to quit and reopen Recordit after changing Screen Recording access.`

### Model Setup

1. `Model required`
2. `Choose a local model file to enable live transcription.`
3. `Model path not found`
4. `Choose model`
5. `Use record-only mode`

### Runtime Status and Transcript

1. `Preparing session`
2. `Listening`
3. `Finalizing session`
4. `Session complete`
5. `Session completed with warnings`
6. `Session failed`
7. `Live transcript`
8. `Waiting for stable transcript lines`

### Trust, Degraded, and Failure Messaging

1. `Quality may be reduced for part of this session.`
2. `Some transcript segments were recovered after backlog pressure.`
3. `Review trust notices before sharing this transcript.`
4. `Recordit could not start this run.`
5. `Try the recommended steps and run again.`

### Export and Privacy

1. `Export transcript`
2. `Export session bundle`
3. `Export diagnostics`
4. `Include transcript text in diagnostics`
5. `Include audio in diagnostics`
6. `Diagnostics may include sensitive content.`

## Localization-Readiness Policy

1. English is the source language (`en`).
2. All keys in the catalog must include comments describing usage intent.
3. New locales must reuse existing keys and only add localizations.
4. Key stability is part of compatibility: avoid key churn across releases.
