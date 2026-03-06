# bd-1e1h: Real Recordit.app Runtime Wiring Closeout

Date: 2026-03-05
Related bead: `bd-1e1h`

## Feature Outcome

`Recordit.app` is now a launchable macOS app target with real app-shell/runtime wiring in the window lifecycle.

This closeout aggregates shipped child lanes and validates the end-to-end app-target build + launch behavior.

## Child Lane Completion Evidence

The concrete implementation lanes under `bd-1e1h` are closed:

- `bd-1nqb` (`CLOSED`): build-system strategy locked to Xcode app-target + `xcodebuild`
- `bd-3vwh` (`CLOSED`): `Recordit.xcodeproj` + `@main` app target created
- `bd-5vt8` (`CLOSED`): first-launch bootstrap hooks for permissions/model readiness validated
- `bd-2hht` (`CLOSED`): runtime intent/service wiring validated
- `bd-vhuq` (`CLOSED`): AppEnvironment/AppShell root composition wired with visible route transitions
- `bd-ph9o` (`CLOSED`): make wrappers for Recordit build/run path validated

## App-Target Validation (This Session)

Build:

```bash
xcodebuild \
  -project Recordit.xcodeproj \
  -scheme RecorditApp \
  -configuration Release \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath .build/recordit-derived-data \
  build
```

Observed result:
- `BUILD SUCCEEDED`

Launch proof:

```bash
APP_PATH="$(pwd)/.build/recordit-derived-data/Build/Products/Release/Recordit.app"
open -n "$APP_PATH"
sleep 2
pgrep -x Recordit
pkill -x Recordit
```

Observed result:
- process PID observed for `Recordit`

## Unblock Impact

Closing `bd-1e1h` removes feature-level blocker status for downstream lanes:

- `bd-d987` (packaging cutover)
- `bd-3sko` (XCTest/XCUITest strategy)
- `bd-2bkg` (windowed UI screens)

## Notes

- Remaining open work continues in downstream beads (full UI coverage, test expansion, DMG/release hardening).
- This bead confirms the runtime wiring foundation is now real app-target behavior, not only smoke/module scaffolding.
