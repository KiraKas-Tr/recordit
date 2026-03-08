# bd-3ipk: Offline Pending Transcription Actions in Sessions UI

Date: 2026-03-05
Related bead: `bd-3ipk`

## Delivered

1. Added explicit pending-transcription state surfaces for record-only sessions in:
   - `app/RecorditApp/MainWindowView.swift`
2. Enhanced list rows to show pending transcription state labels for record-only sessions.
3. Added detail-pane `Deferred Transcription` panel with deterministic state messaging and action gating.

## Acceptance Mapping

- Actionable readiness state: `ready_to_transcribe` now shows explicit `Ready` messaging and enables one-click transcribe.
- Blocked/remediation states:
  - `pending_model`: remediation to complete model setup/onboarding prerequisites
  - `transcribing`: explicit in-progress messaging
  - `completed`: explicit completion messaging
  - `failed`: remediation to inspect artifacts/retry context
  - missing sidecar state: explicit not-available message
- One-click transcribe action remains available only when readiness criteria are met.

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

## Notes

- This bead focuses pending/offline transcription readiness/action UX in sessions surfaces.
- Export/delete actions remain downstream in `bd-6dr6`.
