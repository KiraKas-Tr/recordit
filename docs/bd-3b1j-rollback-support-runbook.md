# bd-3b1j: Release Runbook for Rollback and Support Triage

Date: 2026-03-05  
Status: active runbook for GitHub DMG release operations (`bd-twiz`)

## 1. Purpose

Define deterministic release rollback triggers and support handoff behavior for Recordit DMG releases.

This runbook is the operational companion to:
1. `docs/bd-b2qv-release-checklist.md`
2. `docs/bd-1uik-ga-signing-notarization-plan.md`
3. `docs/bd-2snv-diagnostics-schema.md`
4. `docs/bd-1tuy-diagnostics-redaction.md`

## 2. Severity and Alert Triggers

## Sev-1 (Immediate Rollback Decision Required)

Any one trigger is enough:
1. packaged gate regression:
   - `gate-packaged-live-smoke` result `gate_pass=false`
2. release integrity regression:
   - DMG signature verify fails, notarization rejected, or Gatekeeper assessment fails
3. runtime safety regression:
   - repeated runtime crash/timeout on packaged lane for two consecutive validation runs

## Sev-2 (Stabilize + Monitor, Rollback if Persistent)

Any one trigger:
1. trust/degradation includes severe pressure signals (for example `chunk_queue_backpressure_severe`) in two consecutive runs
2. manifest/session summary indicates sustained high drop ratio above current guardrails
3. support tickets indicate reproducible install or launch failure for a new tag

## Sev-3 (Localized Incident, No Immediate Release Rollback)

Examples:
1. single-session recoverable artifact issue without reproduction
2. support request with missing diagnostics bundle fields
3. user-environment-specific permission/setup issue

## 3. Triage Collection Commands

Run from repo root after detection:

```bash
export RELEASE_TAG="v0.1.0-beta.1"
export INCIDENT_ID="inc-${RELEASE_TAG}-$(date -u +%Y%m%dT%H%M%SZ)"
export INCIDENT_ROOT="artifacts/releases/incidents/${INCIDENT_ID}"
mkdir -p "${INCIDENT_ROOT}"/{signals,diagnostics,notes,release}

PACKAGED_GATE_DIR="$(ls -td ~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/gates/gate_packaged_live_smoke/* | head -1)"
cp "${PACKAGED_GATE_DIR}/status.txt" "${INCIDENT_ROOT}/signals/packaged-smoke.status.txt"
cp "${PACKAGED_GATE_DIR}/summary.csv" "${INCIDENT_ROOT}/signals/packaged-smoke.summary.csv"

br show bd-2n4m --json > "${INCIDENT_ROOT}/signals/bd-2n4m-status.json"
```

Optional manifest signal extraction (if manifest available):

```bash
python3 scripts/manifest_signal_extract.py \
  --manifest ~/Library/Containers/com.recordit.sequoiatranscribe/Data/artifacts/packaged-beta/session.manifest.json \
  > "${INCIDENT_ROOT}/signals/manifest-signals.txt"
```

## 4. Rollback Decision Tree

1. Is this Sev-1?
   - yes: move release to rollback flow immediately.
   - no: continue to step 2.
2. Is this Sev-2 reproduced in two consecutive validation runs?
   - yes: enter stabilization mode (kill-switch), then re-evaluate.
   - no: track as Sev-3 support issue.
3. After stabilization mode, are any Sev-1/Sev-2 triggers still present?
   - yes: rollback.
   - no: keep release active with incident monitoring.

## 5. Rollback Procedure (GitHub DMG Lane)

## Step A - Freeze New Promotion

```bash
gh release edit "${RELEASE_TAG}" --draft
gh release view "${RELEASE_TAG}" --json url,isDraft,assets > "${INCIDENT_ROOT}/release/release-state.json"
```

## Step B - Publish Rollback Notice

Create `${INCIDENT_ROOT}/release/rollback-notice.md` with:
1. incident ID
2. impacted release tag
3. user-visible impact
4. immediate user guidance
5. ETA or next checkpoint

## Step C - Asset Rollback Action

If asset removal is required:

```bash
gh release delete-asset "${RELEASE_TAG}" "Recordit-${RELEASE_TAG}.dmg" --yes
gh release view "${RELEASE_TAG}" --json assets > "${INCIDENT_ROOT}/release/release-assets-after-rollback.json"
```

If prior stable tag exists, update notes to point users to last known good release.

## Step D - Stabilization Command for Runtime Incidents

For runtime pressure incidents, run kill-switch mode while incident is active:

```bash
make run-transcribe-app \
  ASR_MODEL=artifacts/bench/models/whispercpp/ggml-tiny.en.bin \
  TRANSCRIBE_ARGS="--live-stream --disable-adaptive-backpressure"
```

Record command output in `${INCIDENT_ROOT}/signals/kill-switch-run.log`.

## 6. Required Diagnostics Bundle Fields (Support Triage Minimum)

Support intake must include a diagnostics export with these required fields in `diagnostics.json`:
1. `schema_version`
2. `kind`
3. `generated_at_utc`
4. `session_id`
5. `include_transcript_text`
6. `include_audio`
7. `artifacts`
8. `redaction_contract`
9. `support_snapshot`

Required nested minimums:
1. `redaction_contract.mode`
2. `redaction_contract.transcript_text_included`
3. `support_snapshot.schema_version`
4. `support_snapshot.manifest_summary`
5. `support_snapshot.counters`

Quick check command:

```bash
jq '{schema_version,kind,generated_at_utc,session_id,include_transcript_text,include_audio,artifacts,redaction_contract,support_snapshot}' diagnostics.json \
  > "${INCIDENT_ROOT}/diagnostics/diagnostics-minimum.json"
```

## 7. Support Handoff Protocol

## Required handoff packet

Every Sev-1/Sev-2 incident handoff must include:
1. incident summary markdown (`${INCIDENT_ROOT}/notes/incident-summary.md`)
2. trigger evidence files from section 3
3. diagnostics minimum JSON from section 6
4. release rollback state JSON from section 5
5. current owner and next checkpoint time (UTC)

## Escalation ownership

| Incident stage | Owner | SLA |
|---|---|---|
| Initial triage | Release Owner | 30 minutes |
| Diagnostics validation | Support Owner | 60 minutes |
| Rollback approval | Release + Security Owners | 60 minutes |
| User-facing update | Support Owner | 90 minutes |

## Communication template

Add this to `${INCIDENT_ROOT}/notes/incident-summary.md`:
1. `incident_id`
2. `release_tag`
3. `severity`
4. `trigger`
5. `decision` (`monitor` | `stabilize` | `rollback`)
6. `commands_run`
7. `evidence_paths`
8. `next_checkpoint_utc`

## 8. Exit Criteria

Incident can be closed only when:
1. root trigger no longer reproduces
2. required gates pass again (where applicable)
3. support handoff packet is complete
4. release status is explicitly documented (`active`, `draft`, or `rolled_back`)

## 9. Completion Criteria for `bd-3b1j`

`bd-3b1j` is complete when:
1. rollback trigger thresholds are explicit
2. rollback decision tree is documented
3. diagnostics/support required fields are concrete and schema-aligned
4. support handoff protocol and owner accountability are explicit
