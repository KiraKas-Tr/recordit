# Staged Rollout, Migration, and Deprecation Checklist (`bd-2ak3`)

Date: 2026-03-02  
Status: active release checklist for Phase G closeout

## 1. Purpose

Provide one operational checklist for shipping the `recordit` operator-first surface while keeping legacy `transcribe-live` compatibility safe and explicit.

This checklist is normative for release decisions in Phase G (`bd-2cst`).

## 2. Rollout Policy Summary

- Canonical operator surface: `recordit` (`run`, `doctor`, `preflight`, `replay`, `inspect-contract`)
- Legacy compatibility surface: `transcribe-live` remains supported for scripts, gates, packaged wrappers, and expert/debug workflows in this cycle
- Migration style: additive (no forced cutover of legacy automation in this phase)

Primary evidence references:
- `docs/operator-quickstart.md`
- `docs/cli-entrypoint-compat-audit.md`
- `docs/bd-mcva-compat-gate-report.md`
- `docs/runtime-compatibility-boundary-policy.md`

## 3. Staged Rollout Checklist

## Stage A - Operator Surface Readiness

Goal: `recordit` is the obvious first path for humans and agents.

- [x] `README.md` first-run path is `recordit` (not legacy flags)
- [x] operator quickstart published at `docs/operator-quickstart.md`
- [x] state-machine/docs point to canonical `recordit` command grammar
- [x] failure classification and remediation output are deterministic (`run_status`, `remediation_hints`)

Acceptance signal:
- New operator can run `preflight -> run -> replay` without using `transcribe-live` flags first.

## Stage B - Compatibility Surface Lock

Goal: keep legacy surfaces stable where they are still consumed.

- [x] legacy entrypoint compatibility tests are in place (`bd-pu7s`)
- [x] Makefile/scripts/packaged entrypoints are audited and classified (`docs/cli-entrypoint-compat-audit.md`)
- [x] boundary policy documents Tier/S-class compatibility commitments

Acceptance signal:
- Existing compatibility and packaged workflows keep working without migration edits.

## Stage C - Gate Evidence Before Release

Goal: prove new product surface did not invalidate runtime behavior contracts.

- [x] run/verify `make contracts-ci`
- [x] run/verify `make gate-backlog-pressure`
- [x] run/verify `make gate-v1-acceptance`
- [x] run/verify `make gate-transcript-completeness`
- [x] capture and publish summary evidence (`docs/bd-mcva-compat-gate-report.md`)

Acceptance signal:
- all gate verdicts are pass and baseline comparison notes are recorded.

## Stage D - Release Cutover Checklist

Goal: ship intentionally, with explicit fallback.

- [ ] Announce release notes with two-path policy:
  - default operator path = `recordit`
  - legacy compatibility path = `transcribe-live` (supported in this cycle)
- [ ] Confirm no pending P1 open tasks in the rollout chain (`bd-2ak3`, `bd-3h2b`, `bd-2cst`)
- [ ] Confirm command docs and help text contain migration guidance
- [ ] Confirm packaged beta guidance remains aligned with `docs/cli-entrypoint-compat-audit.md`

Go/no-go rule:
- no cutover announcement if compatibility gate evidence is stale, failing, or missing.

## Stage E - Post-Release Observation Window

Goal: confirm rollout remains stable under real operator use.

- [ ] Track trust/degradation and replay support requests in the first observation window
- [ ] Re-run compatibility gate bundle before any follow-up migration step
- [ ] Keep `transcribe-live` path documented as an explicit fallback during the window

Exit condition:
- no unresolved compatibility regressions and no missing migration docs.

## 4. Documentation Switch Points

Use this table when deciding where to direct users during and after release.

| Surface | Current state | Action in this phase |
|---|---|---|
| `README.md` quickstart | `recordit` first | keep as canonical |
| `docs/operator-quickstart.md` | `recordit` canonical | keep as first-run reference |
| `transcribe-live --help` | includes migration guidance | keep guidance and compatibility statement |
| Makefile compatibility/gate targets | mostly legacy-backed | keep unchanged in this cycle |
| packaged smoke and app entrypoints | legacy-backed signed binary | keep unchanged in this cycle |

## 5. Compatibility Fallback and Escalation Plan

If a regression appears, use this sequence:

1. Classify scope with policy docs:
   - `S0/Tier A` contract drift: treat as release-blocking.
   - `S1/S2` migration drift: may ship only with explicit bridge messaging.
2. Reproduce with machine gates (`contracts-ci`, compatibility gate bundle).
3. If `S0/Tier A` break is confirmed:
   - pause rollout communications,
   - keep legacy entrypoint guidance prominent,
   - land minimal compatibility fix before continuing rollout.
4. Publish incident note in the bead/mail thread with:
   - failing gate,
   - affected command surface,
   - remediation owner,
   - rerun evidence after fix.

## 6. Explicit Future Deprecation Conditions for `transcribe-live`

`transcribe-live` is not deprecated in this phase. Any future deprecation proposal must satisfy all conditions below:

1. Replacement coverage:
   - every required automation/gate use case has a supported `recordit`-based equivalent or explicitly accepted legacy retention decision.
2. Evidence durability:
   - compatibility gate suite remains green across at least two consecutive release cycles after migration changes.
3. Documentation completeness:
   - migration guides include old command -> new command mappings for all supported operator workflows.
4. Contract safety:
   - machine-readable contracts/schemas and CI gates are updated with intentional versioning where needed.
5. Release governance:
   - deprecation notice is pre-announced with timeline, fallback, and rollback path.

If any condition fails, `transcribe-live` remains a supported compatibility surface.

## 7. Execution Checklist for Session Closeout

Before closing rollout beads:

1. `br` status updates are accurate (`in_progress`/`closed`)
2. `br sync --flush-only` has been run after issue updates
3. evidence docs are committed with the corresponding bead changes
4. agent-mail thread includes completion summary and links to evidence docs

