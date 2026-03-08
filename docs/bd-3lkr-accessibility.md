# bd-3lkr: Accessibility and Keyboard-First UX Hardening

## Goal

Add explicit accessibility and keyboard metadata across onboarding, runtime, session detail/playback, and export surfaces so UI integrations have deterministic labels, focus order, and shortcut contracts.

## Delivered

1. `app/Accessibility/AccessibilityContracts.swift`
2. `app/AppShell/ModelSetupViewModel.swift`
3. `app/AppShell/PreflightViewModel.swift`
4. `app/ViewModels/RuntimeViewModel.swift`
5. `app/ViewModels/SessionDetailViewModel.swift`
6. `app/ViewModels/SessionPlaybackViewModel.swift`
7. `app/ViewModels/SessionListViewModel.swift`
8. `app/ViewModels/session_list_smoke.swift`
9. `app/Exports/SessionExportService.swift`
10. `app/Accessibility/accessibility_smoke.swift`

## What Landed

1. Added shared accessibility contract types:
- `AccessibilityElementDescriptor`
- `KeyboardShortcutDescriptor`
- `KeyboardFocusPlan`

2. Added deterministic accessibility catalogs for onboarding surfaces:
- `ModelSetupViewModel.onboardingAccessibilityElements`
- `ModelSetupViewModel.onboardingFocusPlan`
- `ModelSetupViewModel.onboardingKeyboardShortcuts`
- `PreflightViewModel.accessibilityElements`
- `PreflightViewModel.focusPlan`
- `PreflightViewModel.keyboardShortcuts`

3. Added runtime/session accessibility metadata:
- `RuntimeViewModel.accessibilityElements/focusPlan/keyboardShortcuts`
- `SessionListViewModel.accessibilityElements/focusPlan/keyboardShortcuts`
- `SessionDetailViewModel.accessibilityElements/focusPlan/keyboardShortcuts`
- `SessionPlaybackViewModel.accessibilityElements/focusPlan/keyboardShortcuts`

4. Added export accessibility metadata:
- `SessionExportAccessibilityCatalog.elements/focusPlan/keyboardShortcuts`

5. Added smoke coverage for metadata integrity:
- non-empty labels/hints/action summaries
- unique element IDs per surface
- deterministic focus order matching element declarations
- non-empty keyboard shortcut bindings per surface

6. Added sessions-list smoke assertions for keyboard-first accessibility contract presence.
