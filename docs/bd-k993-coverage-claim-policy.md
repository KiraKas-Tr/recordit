# bd-k993 — Truthful Coverage-Claim Terminology Policy

## Purpose

This policy prevents the repo from making confidence claims that exceed the evidence we actually retain.

It defines:
- the allowed vocabulary for describing test and verification status
- the minimum proof required before stronger claims are permitted
- the wording that is explicitly disallowed unless the matrix and evidence lanes support it
- a lightweight review checklist that future docs, runbooks, release notes, and gates can cite

This policy is intentionally strict for certification-style statements and practical for day-to-day engineering updates.

## Scope

Applies to:
- repository docs and runbooks
- release notes and acceptance summaries
- verification checklists and evidence reports
- CI/gate output that summarizes project readiness or coverage posture

This policy does **not** ban mocks, fixtures, or scripted lanes. It bans overstating what those lanes prove.

## Canonical Claim Levels

Use the strongest phrase that is still true.

| Claim level | When it is allowed | What it means | What it does **not** mean |
|---|---|---|---|
| `logic-covered` | unit/smoke tests prove narrow logic or mapping behavior | specific code paths are exercised in isolation | not an app journey, integration proof, or release proof |
| `fixture-covered` | frozen JSONL/WAV/model fixtures or deterministic canned payloads are the main evidence | contract parsing or deterministic transformation behavior is covered | not production-runtime realism |
| `simulation-covered` | scripted services, preview DI, `RECORDIT_UI_TEST_MODE`, fake capture, `/usr/bin/true`, or similar seams remain in the lane | meaningful behavior is exercised, but with realism seams | not real-environment verification |
| `temp-filesystem integration-covered` | real service/process contracts run against temp directories/files/binaries | multiple components interact through realistic boundaries | not shipped-artifact or real-device proof |
| `packaged-path verified` | the signed/bundled app path is exercised with retained artifacts/logs | the packaged artifact posture was validated | not DMG install proof or fully real capture proof unless those were part of the lane |
| `real-environment verified` | retained evidence shows the real app/runtime/device environment without the known simulation seams | the described surface was proven in a production-real context | not blanket proof for unrelated surfaces |
| `partial` | some evidence exists, but a major part of the user-facing or release-facing claim is still missing | the surface has progress, but cannot support a strong claim yet | not coverage closure |
| `uncovered` | no canonical lane proves the surface | there is still a direct gap | not an acceptable basis for readiness claims |

## Required Terminology

Prefer these exact phrases when summarizing status:
- `logic-covered`
- `fixture-covered`
- `simulation-covered`
- `temp-filesystem integration-covered`
- `packaged-path verified`
- `real-environment verified`
- `partial`
- `uncovered`
- `covered-with-seams`
- `mock-backed`
- `fake-capture`
- `preview-DI-backed`
- `UI-test-mode-backed`

When realism seams exist, name them directly instead of hiding them behind generic success language.

Examples:
- "The onboarding flow is `simulation-covered` via `RECORDIT_UI_TEST_MODE`, but not yet `real-environment verified`."
- "Packaged live smoke is `packaged-path verified`, but DMG install/open remains `partial`."
- "Runtime status mapping is `logic-covered`; it is not evidence of end-to-end readiness."

## Disallowed Claims Unless Elevated Proof Exists

Do **not** use these phrases unless the prerequisites in the next section are satisfied for the exact surface being described:
- `full coverage`
- `fully covered`
- `completely tested`
- `fully tested`
- `fully verified`
- `production-ready` as a coverage conclusion
- `end-to-end verified` without retained evidence showing the whole claimed journey
- `release-ready` when the evidence is still docs-only, fixture-only, or simulation-only

Also disallowed:
- using a unit/smoke pass to imply app-journey proof
- using scripted UI lanes to imply production-environment proof without naming the seam
- using packaged smoke to imply DMG install/open proof
- using docs/runbooks as if they were execution evidence

## Elevation Rules For Strong Claims

The phrases below are allowed only when **all** conditions are met for the named surface.

### "real-environment verified"

Allowed only when:
1. the lane runs without the known seam classes (`MockServices`, preview-only DI, scripted runtime/preflight replacements, `RECORDIT_UI_TEST_MODE`, fake capture, placeholder binaries)
2. retained artifacts/logs exist and are inspectable
3. the exact surface appears as `real-environment verified` or equivalent in the current matrix/gap-report contract

### "end-to-end verified"

Allowed only when:
1. the claim names one concrete journey or release surface
2. the lane covers the whole stated journey, not a partial slice
3. retained evidence exists from start to finish
4. no uncovered or partial portion of that same named journey is omitted from the claim

### "fully verified" / "full coverage" / "completely tested"

Allowed only when:
1. every critical surface in the relevant scope is no worse than `real-environment verified` or an explicitly accepted equivalent
2. no row in the governing matrix for that scope remains `partial`, `uncovered`, or `covered-with-seams` without an explicit downgrade in the claim
3. required packaged/release/installation evidence exists where the scope implies release confidence
4. the statement names the scope precisely (for example, a specific subsystem), rather than implying the whole product by accident

If any one of these conditions fails, use a narrower phrase.

## Review Checklist

Before publishing any summary that sounds like a confidence claim, answer these questions:

1. What exact surface or journey is being claimed?
2. Does the current matrix mark it as `logic-covered`, `simulation-covered`, `partial`, `uncovered`, or `real-environment verified`?
3. Are there still realism seams? If yes, are they named explicitly in the sentence?
4. Is there retained evidence for the claimed surface, or only docs/manual steps?
5. Does the wording accidentally imply DMG/install/release proof when only local or packaged smoke proof exists?
6. Could a reader mistake this for a whole-product claim when it is only a subsystem claim?

If any answer is uncomfortable or ambiguous, weaken the claim language.

## Mechanical Gating Guidance

Future automation/review work should treat the following as policy violations unless a scope-specific exception exists:
- strong phrases such as `full coverage`, `fully verified`, or `completely tested` appearing in docs/runbooks while linked matrix rows remain `partial`, `uncovered`, or `covered-with-seams`
- release/readiness summaries that omit named realism seams from the underlying lane
- evidence summaries that treat docs/manual instructions as execution proof
- claims that do not name a scope but imply project-wide completeness

## Exception Policy

Exceptions must be explicit and temporary.

Any exception should include:
- scope being exempted
- why stronger wording is temporarily necessary
- owner
- expiry/revisit date
- linked bead that removes the exception

Absence of an exception means the standard terminology policy applies.

## Recommended Short Forms

Use these templates in status updates:
- "`<surface>` is `logic-covered`, but not yet `real-environment verified`."
- "`<surface>` is `simulation-covered` via `<named seam>`; do not treat this as release proof."
- "`<surface>` remains `partial`; see `<bead-id>` for the missing real-evidence lane."
- "`<surface>` is `packaged-path verified`, but DMG/install proof is still out of scope for this claim."

## Related Sources

This policy aligns with:
- `docs/bd-39i6-critical-path-test-realism-inventory.md`
- `docs/bd-39jy-mock-fixture-census.md`
- `docs/bd-11vg-critical-surface-gap-report.md`

## Decision

Until the matrix and evidence lanes improve, the repository should prefer precise downgrade language over broad confidence language.

In short:
- meaningful coverage may exist without being fully real
- packaged verification may exist without install proof
- docs may guide execution without counting as evidence
- no one should claim `full coverage` or `fully verified` unless the graph and artifacts actually support it
