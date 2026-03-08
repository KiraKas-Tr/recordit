# bd-1vo3: Gatekeeper and Notarization Expectations by Validation Context

Date: 2026-03-07  
Related bead: `bd-1vo3`  
Parent feature: `bd-34vh`  
Policy refs:
- `docs/bd-1mep-v1-release-posture.md`
- `docs/bd-3p8a-release-context-matrix.md`
- `docs/bd-1huk-release-artifact-inspection.md`

## Purpose

Prevent context drift when interpreting `spctl` output. Local development checks and
notarized release-candidate checks answer different questions and are not interchangeable.

## Validation Modes

| Mode | Primary claim | Canonical command path | Authoritative evidence root |
|---|---|---|---|
| Local ad-hoc / packaged-local validation | `dist/Recordit.app` + local DMG are structurally correct and runtime payload checks pass | `make inspect-recordit-release-artifacts` (+ optional `make gate-dmg-install-open`) | `artifacts/ops/release-artifact-inspection/<stamp>/` |
| Notarized release-candidate validation | distributable DMG is signed, notarized, stapled, and Gatekeeper-assessed | `make notarize-recordit-dmg ...` on the RC DMG | `artifacts/releases/notary/<stamp-or-tag>/` |

## Mode A: Local Ad-Hoc / Packaged-Local Validation

Use this for daily engineering and packaged-local verification.

```bash
make inspect-recordit-release-artifacts \
  RECORDIT_DMG_NAME=Recordit-local.dmg \
  RECORDIT_DMG_VOLNAME='Recordit'

# Optional retained install/open evidence lane:
make gate-dmg-install-open \
  RECORDIT_DMG_NAME=Recordit-local.dmg \
  RECORDIT_DMG_VOLNAME='Recordit'
```

### Required local evidence checks

```bash
LOCAL_ROOT="$(ls -td artifacts/ops/release-artifact-inspection/* | head -1)"

test -f "$LOCAL_ROOT/status.txt"
test -f "$LOCAL_ROOT/summary.csv"
test -f "$LOCAL_ROOT/dist_release_context/summary.csv"
```

Interpretation rules for local `spctl`:
1. In this mode, `spctl` rows may be `warn` (or `fail` for unsigned artifacts) and still be
   valid local evidence for packaging/debug iteration.
2. A common local non-notarized signal is text like `rejected` or `source=no usable signature`.
3. Treat local `spctl` output as *diagnostic context*, not ship-readiness proof.

## Mode B: Notarized Release-Candidate Validation

Use this mode for shipping claims.

```bash
export RELEASE_TAG="v0.1.0-rc.1"
export SIGN_IDENTITY="Developer ID Application: <team>"
export NOTARY_PROFILE="<notarytool-keychain-profile>"
export NOTARY_ROOT="artifacts/releases/notary/${RELEASE_TAG}"

make sign-recordit-app SIGN_IDENTITY="$SIGN_IDENTITY"
make create-recordit-dmg \
  RECORDIT_DMG_NAME="Recordit-${RELEASE_TAG}.dmg" \
  RECORDIT_DMG_VOLNAME='Recordit' \
  SIGN_IDENTITY="$SIGN_IDENTITY"

make notarize-recordit-dmg \
  RECORDIT_DMG_NAME="Recordit-${RELEASE_TAG}.dmg" \
  SIGN_IDENTITY="$SIGN_IDENTITY" \
  NOTARY_PROFILE="$NOTARY_PROFILE" \
  OUT_DIR="$NOTARY_ROOT"
```

### Required release evidence checks

```bash
test -f "$NOTARY_ROOT/status.txt"
test -f "$NOTARY_ROOT/summary.csv"
test -f "$NOTARY_ROOT/notary/notary-submit.json"
test -f "$NOTARY_ROOT/notary/notary-log.json"
test -f "$NOTARY_ROOT/logs/stapler_validate.log"
test -f "$NOTARY_ROOT/logs/spctl_assess.log"

jq -r '.status' "$NOTARY_ROOT/notary/notary-submit.json"
rg '^notary_status,pass,' "$NOTARY_ROOT/summary.csv"
rg '^spctl_assess,pass,' "$NOTARY_ROOT/summary.csv"
```

Interpretation rules for release `spctl`:
1. `spctl_assess=pass` is required for distributable RC/GA posture.
2. `ALLOW_SPCTL_FAILURE=1` is a temporary waiver mechanism only; do not use it as release proof.
3. Notarization + stapling + Gatekeeper pass are the authoritative shipping gate for this scope.

## Anti-Drift Guardrails

1. Never cite Mode A evidence as proof of Mode B readiness.
2. If Mode B fails, reopen/fix in Mode B; do not downgrade the claim to Mode A.
3. Keep release claims aligned with v1 posture: DMG-distributed, hardened-runtime, notarized,
   unsandboxed `Recordit.app`.

