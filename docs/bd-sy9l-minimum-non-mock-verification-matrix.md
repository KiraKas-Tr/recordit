# bd-sy9l — Minimum non-mock verification matrix for critical journeys

Date: 2026-03-07  
Related bead: `bd-sy9l`  
Upstream references:
- `docs/bd-34yb-comprehensive-non-mock-critical-journey-verification-lanes.md`
- `docs/bd-39i6-critical-surface-coverage-matrix.md`
- `docs/bd-2mbp-no-mock-critical-path-policy.md`

## Purpose

Define the minimum verification floor for each critical product journey, including:
- readiness/start behavior
- bundled runtime/model parity
- stop/finalization lifecycle outcomes
- onboarding/remediation
- Record Only fallback
- packaged/release-context validation

This matrix answers: **what must be real, and what cannot be certified by mock-heavy lanes alone?**

## Floor levels

| Floor level | Meaning | Certifies critical-journey claims? |
| --- | --- | --- |
| `unit-only` | logic/contracts only | no |
| `fixture-only` | deterministic fakes or synthetic artifacts | no |
| `packaged-app` | signed `dist/Recordit.app` boundary proof | partially, only for packaged-local scope |
| `live-real` | production wiring, real runtime boundary, no disallowed seams | yes |

## Disallowed seams for certifying claims

The following may still be useful for diagnostics/regression speed, but do not satisfy non-mock certification by themselves:
- `RECORDIT_UI_TEST_MODE`
- `AppEnvironment.preview()`
- mock/stub/scripted service replacements on the claimed critical path
- placeholder runtime binaries or compatibility-only bypasses
- fake capture fixtures when the claim is real live behavior
- synthetic manifests/session roots used as substitutes for primary journey output

## Minimum matrix

| Journey | Required verification floor | Why mocks/fakes alone are insufficient | Required lane integration |
| --- | --- | --- | --- |
| Startup readiness and live start gating | `live-real` | readiness authority and action gating must match real runtime/model state | `bd-tr8z` (readiness parity) + packaged readiness evidence (`bd-diqp` outputs where applicable) |
| Bundled runtime/model parity | `packaged-app` + `live-real` for certifying end-user claims | Xcode-only or synthetic bundle checks miss distributed artifact boundary failures | `bd-diqp` (runtime/model parity) + release-context verifier lanes |
| Stop/finalization lifecycle outcomes | `live-real` (plus packaged confirmation for release-path confidence) | timeout tuning and synthetic lifecycle scripts alone can hide real control-boundary failures | `bd-p77p` (coverage), including packaged taxonomy checks (`bd-2kia`) |
| First-run onboarding/remediation | `live-real` | preview/UI-test-mode shortcuts can pass without real first-run wiring | production app-shell/onboarding lanes from the non-mock program (`NM-01`/`NM-02`) |
| Record Only fallback when live is blocked | `live-real` | scripted fallback payloads cannot certify runtime-driven affordance truthfulness | readiness + fallback lanes tied to `bd-tr8z` and runtime ownership work |
| Packaged release-context verification | `packaged-app` minimum; `release-candidate` for shipment claims | docs-only/runbook-only proof cannot certify signed artifact behavior | packaged/release verification lanes (`verify_recordit_release_context.sh`, packaged smoke gates, downstream RC lanes) |

## Certification rules

1. A journey is not certifiable if its highest retained evidence remains `unit-only` or `fixture-only`.
2. A journey requiring `live-real` is not certifiable when disallowed seams are present on the asserted path.
3. `packaged-app` proof certifies packaged-local boundary behavior, not full installed/RC shipment behavior.
4. Strong coverage wording for critical journeys is blocked until all rows above meet their required floor.

## Integration with existing beads

- `bd-tr8z`: provides required readiness parity foundation for startup and fallback rows.
- `bd-diqp`: provides bundled runtime/model parity floor and packaged-boundary proof.
- `bd-p77p`: provides lifecycle control/taxonomy evidence floor for stop/finalization behavior.

This matrix is the minimum floor definition that downstream orchestration (`bd-2t10`) must treat as non-optional.
