# bd-6dr6: Export/Delete Actions With Confirmation and Privacy Copy

Date: 2026-03-05
Related bead: `bd-6dr6`

## Delivered

1. Added concrete session actions panel in sessions detail UI:
   - `app/RecorditApp/MainWindowView.swift`
   - actions: `Export Bundle`, `Export Transcript`, `Export Diagnostics`, `Delete to Trash`
2. Added explicit privacy copy for export operations.
3. Added explicit confirmation prompts before export/delete actions.
4. Added deterministic user-safe outcome messaging for success/failure states.

## Acceptance Mapping

- Export flows exposed in UI with confirmations:
  - bundle/transcript/diagnostics all require explicit user confirmation
- Privacy language is explicit:
  - exports leave app-managed storage (`~/Downloads/RecorditExports`)
  - diagnostics export defaults to redacted transcript/audio behavior
- Delete flow exposed with confirmation and service-layer mapping:
  - `deleteSession(..., confirmTrash: true)` wired through sessions controller
  - UI shows deterministic success/failure outcomes
- Outcomes are user-safe and actionable:
  - success surfaces absolute exported path (and reveals in Finder)
  - failures surface user message returned from service-layer errors

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

- This bead delivers the UI action/confirmation/privacy contract in app-target context.
- Accessibility-control alignment for these concrete controls remains coordinated with `bd-1ud0`.
