# bd-12fg: Sessions List/Detail UI With Filters, Search, Playback

Date: 2026-03-05
Related bead: `bd-12fg`

## Delivered

1. Replaced sessions route placeholder with a real split list/detail UI in:
   - `app/RecorditApp/MainWindowView.swift`
2. Added a `SessionsLibraryController` in-app wiring layer (same file) to bridge:
   - `SessionListViewModel`
   - `SessionDetailViewModel`
   - `SessionPlaybackViewModel`
3. Bound sessions navigation route state to UI selection state:
   - `list` route clears selection
   - `detail(sessionID)` route loads detail + playback context

## Acceptance Mapping

- Sessions list/detail UI: delivered with deterministic newest-first list rendering from `SessionListViewModel.visibleItems`.
- Filter/search controls: delivered via mode/status pickers + transcript/session search text field + clear/refresh actions.
- Playback: delivered with play/pause, seek slider, elapsed/duration labels, unavailable/error surfaces.
- Transcript detail: delivered with stable timeline rendering including deterministic per-line time/channel/event metadata.
- Empty/error states: delivered for list empty, list service failure, detail idle, transcript empty, and playback unavailable/failure.

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

- This bead intentionally covers core sessions list/detail/playback/transcript composition and route binding in real app target context.
- Follow-on actions (`bd-1ud0`, `bd-3ipk`, `bd-6dr6`) remain for deeper accessibility-control binding, pending transcription flows, and export/delete UX actions.
