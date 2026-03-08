# bd-14y4: SequoiaTranscribe Fallback Policy (Non-Default)

## Policy Goal

Keep `Recordit.app` as the only user-facing default path while preserving a tightly controlled fallback path for `SequoiaTranscribe.app` during diagnostics and incidents.

This policy is mandatory for release docs, support guidance, and operational runbooks.

## Default vs Fallback Contract

### Default (allowed for normal users)

- `Recordit.app` launch/install flow
- Recordit-first packaging, signing, notarization, and gate checks

### Fallback (allowed only under controlled conditions)

- `SequoiaTranscribe.app` with explicit arguments (for diagnostics or incident mitigation)
- terminal/operator-invoked compatibility commands (for example `make run-transcribe-app`)

### Explicitly prohibited

- presenting `SequoiaTranscribe.app` as default app in installer UX, docs, or support scripts
- directing end users to double-click `SequoiaTranscribe.app` for routine usage
- silently routing normal user journeys through compatibility path without explicit incident context

## Approved Fallback Scenarios

Use fallback only when at least one condition holds:

1. Sev-1/Sev-2 incident requires temporary runtime continuity while Recordit lane is being repaired
2. targeted engineering diagnostics require compatibility executable behavior not yet available in Recordit path
3. release rollback rehearsal explicitly includes compatibility validation as a bounded check

Every fallback invocation must include:

- incident/bead/thread reference ID
- explicit reason for fallback activation
- expected exit condition back to Recordit-default flow

## Escalation Triggers

Escalate to incident posture immediately if any trigger is observed:

1. repeated user-facing launch failures on `Recordit.app` that require compatibility workaround
2. support instructions begin referencing SequoiaTranscribe for non-incident routine use
3. packaged gate evidence fails to prove Recordit-default launch semantics
4. fallback usage frequency exceeds one release cycle without an approved extension

Required escalation actions:

1. open/attach incident thread and assign owner
2. publish temporary workaround with explicit sunset deadline
3. block GA promotion until Recordit-default path is restored and verified

## Deprecation Timeline

### Phase A (current)

- `SequoiaTranscribe` remains compatibility-only with explicit non-default labeling
- unsupported no-arg launch path must show guidance to `Recordit.app`

### Phase B (next release milestone)

- remove fallback references from user-facing onboarding/install docs
- retain fallback only in engineering/support runbooks
- require approval from release owner for each fallback use

### Phase C (GA hardening)

- compatibility path disabled for routine operator flows
- fallback available only behind incident-control procedures
- default-path regression checks become release-blocking gates

### Phase D (sunset target)

- remove remaining compatibility shim/automation dependencies once migration evidence is green
- archive policy as historical artifact after shim retirement

## Ownership and Accountability

- Product/UX owner: ensure public messaging never positions fallback as default
- Release owner: enforce gate/no-go checks and fallback approval controls
- Support owner: use fallback only with incident IDs and documented exit conditions
- Engineering owner: maintain compatibility path only as long as approved by timeline gates

## Verification Checklist

Use this checklist before release sign-off:

1. README and release docs label SequoiaTranscribe as non-default fallback only
2. compatibility launches require explicit args and show clear no-arg guidance
3. Recordit-default packaging/gate evidence is current and green
4. active fallback uses (if any) have incident IDs, owners, and sunset dates

## Related Artifacts

- `docs/adr-005-recordit-default-entrypoint.md`
- `docs/bd-1uik-ga-signing-notarization-plan.md`
- `docs/bd-b2qv-release-checklist.md`
- `docs/bd-vsix-no-arg-guidance.md`
