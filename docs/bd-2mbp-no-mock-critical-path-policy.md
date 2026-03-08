# bd-2mbp — No-Mock Critical-Path Policy and Exception Register

Date: 2026-03-06
Related bead: `bd-2mbp`
Upstream sources:
- `docs/bd-39jy-mock-fixture-census.md`
- `docs/bd-39i6-critical-surface-coverage-matrix.md`
- `docs/bd-39i6-canonical-downstream-matrix.md`
- `docs/bd-11vg-critical-surface-gap-report.md`

Machine-readable companion:
- `docs/bd-2mbp-critical-path-exception-register.csv`

## Purpose

Define a repo policy that is deliberately strict for critical-path coverage claims without turning normal unit testing into ceremony.

This policy does **not** ban mocks, stubs, fixtures, preview DI, fake capture, temp filesystems, or scripted runtime helpers across the whole repository. It does require that:

1. critical-path claims stop pretending seam-heavy lanes are equivalent to real-environment proof
2. every temporary critical-path seam is tracked in one explicit exception register
3. downstream scanner and CI work has stable machine-readable inputs instead of prose-only guidance

## Core Rule

A critical-path surface may rely on seams for bounded regression coverage, but it may not be described as fully covered, complete, production-real, live-real, or release-certified unless the strongest lane for that surface is free of the registered seams that cap the claim.

In short:

- mocks and seams are allowed for local logic and bounded regression confidence
- mocks and seams are **insufficient by default** for critical-path completeness claims
- any critical-path seam that remains temporarily tolerated must appear in the exception register with owner, rationale, expiry, and replacement plan

## Critical-Path Scope

This policy applies to the critical surfaces enumerated in `docs/bd-39i6-canonical-downstream-matrix.md`, especially any surface whose current `gap_status` is `covered-with-seams` or `partial` and whose `main_bypass_or_limit` includes one or more of:

- `RECORDIT_UI_TEST_MODE`
- `AppEnvironment.preview()`
- `Mock*`, `Stub*`, `Static*`, or `Scripted*` service substitution
- `/usr/bin/true` or similar runtime/model overrides
- `RECORDIT_FAKE_CAPTURE_FIXTURE`
- frozen JSON/JSONL/WAV fixtures standing in for user-real execution
- temp-filesystem-only session roots when the product claim is app-install or user-journey scoped

## Claim Tiers

| claim tier | what it means | seam-bearing lanes allowed? | notes |
|---|---|---|---|
| `logic-local` | correctness inside one module or one bounded service | yes | unit and narrowly scoped smoke coverage can use mocks/fixtures freely if the claim stays local |
| `service-level` | multi-component behavior with real parsing/io/process boundaries | yes, but must be named | temp-filesystem and fixture seams are acceptable if the lane is described as bounded/service-level |
| `product-journey` | the user-visible app path from launch to outcome | no, unless explicitly downgraded and registered | seam-bearing product-journey claims must be called `covered-with-seams`, never complete |
| `release-surface` | packaged app, DMG, signing, notarization, install/open behavior | only for bounded subclaims | packaged checks alone cannot stand in for install/open or live capture truth |
| `certification` | wording such as full, complete, production-real, release-ready, fully verified | no | any active critical-path exception blocks this wording |

## Layer Policy

| layer | seams normally acceptable | still not enough for |
|---|---|---|
| `unit` | mocks, stubs, fixtures, preview DI, fake capture, temp filesystems | app journey proof, release posture, complete coverage claims |
| `integration` | fixtures and temp filesystems if the claim is explicit and bounded | production environment truth, live TCC behavior, installed-app confidence |
| `scripted-e2e` | deterministic fixtures, fake capture, scripted runtime, temp filesystems | live-real device/TCC behavior, shipped-app truth, install/open truth |
| `xctest` | preview DI and injected services for app logic/state checks | production app/runtime parity or complete readiness proof |
| `xcuitest` | UI-test-mode and scripted runtime only when labeled | production environment readiness, true runtime ownership, live capture truth |
| `release-script` | packaged payload checks plus deterministic runtime probes | install/open success, true first-run onboarding, or live capture proof by itself |
| `contract-test` / `contract-harness` | frozen artifacts and parser/schema fixtures | user-real product behavior or complete critical-path claims |

## Critical-Path Flows That Require Non-Mock Evidence

The following flow classes require non-mock, non-preview, non-UI-test-mode evidence before they can support certification-level wording:

- `production-app-journey`
- `live-tcc-capture`
- `dmg-install-open`
- `playback-functional`
- `packaged-runtime-lookup`
- `release-signing-notarization` for an actual candidate artifact rather than docs-only procedure

## Exception Register Contract

The companion CSV is the canonical temporary-exception register. Every row must include all of the following fields:

| field | requirement |
|---|---|
| `exception_id` | stable identifier for diffs and scanner output |
| `surface_key` | must match `docs/bd-39i6-canonical-downstream-matrix.csv` |
| `gap_status` | current matrix state for that surface |
| `claim_scope` | narrow claim that remains temporarily allowed |
| `lane_id` | current strongest lane carrying the seam |
| `seam_family` | normalized seam vocabulary from `docs/bd-39jy-mock-fixture-census.md` |
| `seam_detail` | concrete bypass or limit, usually copied from `main_bypass_or_limit` |
| `owner_area` | accountable module or surface owner; blank owners are invalid |
| `rationale` | why the temporary seam is still tolerated |
| `temporary_allowed_claim` | strongest claim still allowed while the exception is active |
| `prohibited_claim` | wording that remains disallowed |
| `replacement_bead` | bead tracking the next concrete move toward retirement or scope reduction |
| `replacement_condition` | observable condition for removing the row |
| `created_at_utc` | explicit creation date |
| `expires_at_utc` | explicit expiry date; permanent exceptions are not allowed for critical-path seams |
| `status` | one of `active`, `replacement_in_progress`, `expired`, `retired` |
| `notes` | free-form detail for audits and review context |

## Enforcement Rules

1. **No silent critical-path seams.** If a critical-path surface has a seam-bearing strongest lane and no matching active register row, downstream scanner/CI work should fail.
2. **No ownerless exceptions.** Blank `owner_area` or missing `replacement_bead` should fail validation.
3. **Transition-state rows must say so explicitly.** If a stronger lane already landed but docs or matrix normalization still lag, the row may stay `replacement_in_progress` with notes that explain the remaining downgrade or retirement step.
4. **No expired exceptions in green policy mode.** Expired rows should become failures, not warnings, unless a scanner has a clearly named bootstrap mode.
5. **No certification wording while active exceptions remain.** Any active row blocks claims such as `full coverage`, `complete e2e coverage`, `fully verified`, `production-real`, or equivalent wording for that scope.
6. **New seam families require both docs and register updates.** If a new bypass pattern appears in a critical-path lane, `bd-1jc9` should treat it as drift until both the census vocabulary and the exception register are updated.
7. **Retiring an exception requires evidence, not intention.** A row may move to `retired` only after the replacement lane exists and the matrix/gap report no longer needs the old seam as the strongest proof.

## Initial Register Interpretation

The seeded register is intentionally broad enough to cover the currently known seam-bearing critical surfaces so downstream automation can start from a truthful baseline instead of failing on the entire existing matrix.

Interpret it this way:

- active rows are not endorsements of realism; they are admissions of bounded temporary dependence
- the presence of a row limits what can be claimed
- deleting a row without replacing the underlying lane is policy drift

## Downstream Use

- `bd-1jc9` should parse `docs/bd-2mbp-critical-path-exception-register.csv` directly and fail on missing/expired/unowned rows.
- `bd-2ptr` should use the register to identify which UI-test-mode, preview-DI, and runtime-override seams still need anti-bypass proof.
- `bd-3p9b` should block `full` or `complete` wording while any `active` or `replacement_in_progress` row remains for the relevant critical surface.
- docs and reviews should use `covered-with-seams` or `partial` terminology whenever a matching exception row is still active.

## Practical Reading

The policy line is strict but simple:

- use seams freely for local correctness
- use seams cautiously for service-level or bounded regression proof
- register seams explicitly for critical-path surfaces
- do not let seam-bearing lanes masquerade as complete product truth
