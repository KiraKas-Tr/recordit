# bd-2ooi: Four-Screen Onboarding Flow

Date: 2026-03-05
Related bead: `bd-2ooi`

## Delivered

1. Added explicit four-screen onboarding flow view:
   - `app/RecorditApp/OnboardingFlowView.swift`
   - screens: `Welcome`, `Permissions`, `Model Setup`, `Ready`
2. Replaced single onboarding panel with routed onboarding flow:
   - `app/RecorditApp/MainWindowView.swift`
3. Extended root snapshot/controller data bindings so onboarding UI can consume realistic app-shell states:
   - `PreflightViewModel.State`
   - `ModelSetupViewModel.State`
   - selected backend + diagnostics
   - onboarding gate failure and completion readiness
4. Updated app project target sources to include onboarding flow view:
   - `Recordit.xcodeproj/project.pbxproj`

## Acceptance Mapping

- Four onboarding screens exist and are connected via deterministic back/next transitions.
- Permissions screen is bound to preflight permission checks and displays realistic readiness/failure details.
- Model setup screen is bound to backend selection and model validation state/diagnostics.
- Ready screen enforces completion gating via app-shell model/preflight readiness.
- Completion action routes through `AppShellViewModel.completeOnboardingIfReady(...)` and transitions to main runtime when gates are satisfied.

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

- This bead focuses onboarding surface composition and route connectivity.
- Full onboarding automation/XCUITest coverage remains downstream (`bd-1aqk`, `bd-3sko`, `bd-8du2`).
