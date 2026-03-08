# bd-20uu: MainSessionView Runtime Screen

Date: 2026-03-05
Related bead: `bd-20uu`

## Delivered

1. Added `MainSessionView` and `MainSessionController` in:
   - `app/RecorditApp/MainSessionView.swift`
2. Wired main runtime route to render the new screen from root composition:
   - `app/RecorditApp/MainWindowView.swift`
3. Expanded project target sources to include the new runtime screen file:
   - `Recordit.xcodeproj/project.pbxproj`

## Runtime Screen Behavior

The main runtime route now exposes:

1. Mode selector (segmented):
   - `Live Transcribe`
   - `Record Only`
2. Runtime status badge and elapsed timer.
3. Start/Stop controls with state-aware enable/disable behavior.
4. Transcript panel with timestamped session events (start/stop/failure lifecycle messaging).
5. Output-root visibility and recovery/remediation surface when runtime errors occur.

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

- Runtime process orchestration remains in existing service/viewmodel layers (`RuntimeViewModel`, `RuntimeService`) per architecture boundary rules.
- This bead focuses on main runtime screen composition and intent binding, not full transcript stream decoding UX parity.
