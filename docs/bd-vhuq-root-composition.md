# bd-vhuq: AppEnvironment/AppShell Root Composition Wiring

Date: 2026-03-05
Related bead: `bd-vhuq`
Depends on: `bd-3vwh`, `bd-1e1h`

## Delivered

1. Replaced placeholder `MainWindowView` with a real root composition view driven by:
   - `AppShellViewModel` route state
   - `AppNavigationCoordinator` intents
   - `AppEnvironment` dependency injection surface
2. Added route-visible UI transitions for:
   - onboarding
   - main runtime
   - sessions (list/detail path visibility)
   - recovery
3. Bound onboarding actions into app-shell gate flow:
   - run preflight
   - acknowledge warnings
   - validate model setup
   - complete onboarding (gated)
4. Expanded `RecorditApp` target source membership so real app-target compilation includes non-smoke app modules (`AppShell`, `Navigation`, `Services`, `RuntimeProcessLayer`, `ViewModels`, etc.).

## Files Updated

- `app/RecorditApp/MainWindowView.swift`
- `app/RecorditApp/RecorditApp.swift`
- `Recordit.xcodeproj/project.pbxproj`

## Deterministic Validation

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

Observed result in this session:
- `BUILD SUCCEEDED`

Launch proof:

```bash
APP_PATH="$(pwd)/.build/recordit-derived-data/Build/Products/Release/Recordit.app"
open -n "$APP_PATH"
sleep 2
pgrep -x Recordit
pkill -x Recordit
```

Observed result in this session:
- app process PID observed (`Recordit` launched)

## Notes

- This bead wires root composition and visible navigation transitions only.
- Deeper screen implementation beads (`bd-2ooi`, `bd-12fg`, `bd-20uu`) still own full route-specific UI content.
