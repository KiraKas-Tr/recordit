# bd-31kn: SessionSummarySheet and ErrorRecoverySheet

Date: 2026-03-05
Related bead: `bd-31kn`

## Delivered

1. Added runtime terminal sheet surfaces in:
   - `app/RecorditApp/MainWindowView.swift`
   - `SessionSummarySheet`
   - `ErrorRecoverySheet`
2. Wired terminal sheet presentation to runtime terminal states (`completed`/`failed`) via `mainSessionController.runtimeState` changes.
3. Extended `MainSessionController` to expose manifest-backed finalization summary and deterministic recovery action hooks in:
   - `app/RecorditApp/MainSessionView.swift`

## Acceptance Mapping

- Summary surface appears on terminal completion and shows manifest-driven status/trust fields (`status`, `trustNoticeCount`, `manifestPath`).
- Recovery surface appears on runtime failure and shows actionable controls derived from `RuntimeViewModel.suggestedRecoveryActions`.
- Primary actions are deterministic and wired to concrete handlers:
  - `resumeSession` -> resume interrupted session
  - `safeFinalize` -> safe finalize interrupted session
  - `retryStop` / `retryFinalize` -> retry stop/finalization path
  - `openSessionArtifacts` -> open current session output root
  - `runPreflight` -> route to onboarding/preflight lane
  - `startNewSession` -> route to main runtime lane

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

- The summary sheet now closes the runtime completion loop with explicit session outcome messaging and trust/degradation visibility.
- The recovery sheet converts previously passive failure labels into direct, deterministic recovery controls suitable for UI automation follow-on work (`bd-2ghc`).
