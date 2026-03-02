# Staged Rollout, Migration, and Deprecation Checklist (`bd-2ak3`)

Date: 2026-03-02  
Status: canonical rollout checklist for switching the primary operator surface to `recordit` while preserving legacy `transcribe-live` compatibility

## Purpose

This checklist turns the productization work into an explicit release sequence.

It answers four operational questions:

1. What changes now?
2. What remains supported and stable this cycle?
3. What evidence must stay green to keep shipping?
4. What conditions must be met before any stronger deprecation step for `transcribe-live`?

## Current Decision Boundary

Primary operator surface:

- `recordit` is the canonical human-first shell for:
  - `run --mode live|offline`
  - `doctor`
  - `preflight`
  - `replay`
  - `inspect-contract`

Legacy-stable compatibility surface for this cycle:

- `src/bin/transcribe_live.rs`
- Make debug/runtime targets:
  - `transcribe-live`
  - `transcribe-live-stream`
  - `transcribe-preflight`
  - `transcribe-model-doctor`
  - `capture-transcribe`
- compatibility gate wrappers:
  - `gate-backlog-pressure`
  - `gate-transcript-completeness`
  - `gate-v1-acceptance`
  - `gate-d-soak`
  - `gate-packaged-live-smoke`
- packaged app wrappers around `SequoiaTranscribe.app`

These boundaries are intentionally conservative for this rollout. The operator story moves to `recordit`; the compatibility/gate story remains on `transcribe-live` until a later cycle with explicit migration/versioning.

## Release Sequence

### Stage 0: Freeze and Verify the Compatibility Boundary

- [x] Freeze runtime/public contract inventory.
- [x] Freeze schema/versioning policy and machine-readable contract surfaces.
- [x] Audit Makefile, script, and packaged entrypoint migration boundaries.
- [x] Add explicit legacy entrypoint compatibility coverage.
- [x] Reconfirm operator-path acceptance on the canonical `recordit` flow.
- [x] Reconfirm baseline comparison evidence for:
  - representative offline/chunked artifacts
  - live-stream fake-capture path
  - packaged smoke path
  - transcript completeness gate

Ship gate for this stage:

- `contract_baseline_matrix` passes
- `runtime_manifest_contract` passes
- `make gate-transcript-completeness` passes
- child compatibility comparison beads remain closed with evidence

### Stage 1: Switch Human-First Guidance to `recordit`

- [x] Make README primary examples point to `recordit`.
- [x] Publish operator quickstart centered on `recordit`.
- [x] Align state-machine and agent discovery docs to the final shell/module story.
- [x] Keep legacy `transcribe-live` documentation present only as compatibility/debug/expert guidance.

Definition of done for this stage:

- a new operator can find one obvious happy path without reading legacy flags first
- agent-facing docs identify the canonical shell and contract files without ambiguity

### Stage 2: Ship Dual-Surface Support

- [ ] Treat `recordit` as the default recommended entrypoint for humans and new automation.
- [ ] Keep `transcribe-live` stable for:
  - existing scripts
  - compatibility gates
  - packaged smoke validation
  - expert/debug workflows
- [ ] Preserve all `S0` compatibility commitments documented in:
  - `docs/runtime-compatibility-boundary-policy.md`
  - `docs/runtime-public-contract-inventory.md`

Release note language for this stage:

- "`recordit` is now the primary operator surface."
- "`transcribe-live` remains supported for compatibility, gates, and expert workflows."
- "No deprecation deadline is being enforced in this release."

### Stage 3: Observe and Hold the Bridge Stable

- [ ] Keep compatibility gates in CI/release review.
- [ ] Require fresh evidence before changing:
  - stable CLI flag names
  - runtime mode labels
  - JSONL `event_type` values
  - manifest stable keys/semantics
  - deterministic startup/close-summary ordering
  - gate `summary.csv` / `status.txt` keys used by automation
- [ ] Keep packaged smoke validation on the signed legacy app path until a distinct migration plan exists.
- [ ] Treat help-text wording and docs as evolvable, but not the stable spellings/meanings of `S0` surfaces.

### Stage 4: Consider Stronger `transcribe-live` Deprecation Only After Evidence Exists

Do not escalate beyond "supported compatibility surface" until all of the following are true:

- [ ] at least one full release cycle has shipped with `recordit` as the primary documented path
- [ ] compatibility gates remain green across that cycle
- [ ] packaged smoke or replacement packaged validation is green with no unresolved drift
- [ ] no required CI/release workflow still depends exclusively on raw `transcribe-live` invocation
- [ ] migration messaging is present in help/docs/release notes
- [ ] a fallback bridge remains available for automation users
- [ ] any breaking contract change has an explicit versioning/migration plan

Until those conditions are met, the correct stance is:

- keep `transcribe-live` stable
- keep it documented as compatibility/expert surface
- do not remove flags, rename stable selectors, or repurpose gate wrappers

## Documentation Switch Points

Apply these switch-point rules consistently:

| Surface | This cycle | Rule |
|---|---|---|
| `README.md` primary examples | `recordit` | Show the human happy path first |
| `docs/operator-quickstart.md` | `recordit` | Canonical first-run path |
| `docs/state-machine.md` | `recordit` shell + legacy runtime shell | Normative architecture story |
| `docs/agent-contract-index.md` | canonical contract/discovery map | Point agents to `recordit` and frozen contract docs |
| Make runtime/gate targets | `transcribe-live` / packaged app | Keep stable this cycle |
| packaged smoke docs/scripts | legacy packaged app path | Keep stable this cycle |

## Fallback and Rollback Plan

If any of the compatibility evidence regresses during rollout:

1. Stop expanding the `recordit` migration surface.
2. Keep `transcribe-live` as the compatibility-safe path.
3. Re-run the affected gate/test bundle to isolate whether drift is in:
   - contract schema/output
   - runtime artifact semantics
   - packaged wrapper behavior
   - docs/help mismatch
4. Revert only the newly introduced migration/defaulting behavior, not the frozen compatibility surfaces.
5. Do not update baseline artifacts unless the change is intentionally breaking/additive and documented as such.

Rollback trigger examples:

- `gate_pass=false` in a frozen gate output
- mismatch against frozen baseline matrix expectations
- stable manifest/runtime keys missing or semantically changed
- operator docs claiming `recordit` behaviors that current builds do not deliver

## Exit Criteria for This Bead

This rollout checklist is complete when the team can answer, without ad hoc interpretation:

- what is canonical now
- what is still bridge-supported
- what evidence must remain green
- what would justify or block a future `transcribe-live` deprecation step

## Source Documents

- `docs/cli-entrypoint-compat-audit.md`
- `docs/operator-quickstart.md`
- `docs/runtime-compatibility-boundary-policy.md`
- `docs/runtime-public-contract-inventory.md`
- `docs/bd-3f6g-live-packaged-baseline-report.md`
