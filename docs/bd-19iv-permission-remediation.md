# bd-19iv: Permission Remediation UX Flow

## Goal

Provide deterministic permission recovery behavior for onboarding and recovery surfaces:

1. clear missing-permission identification for Screen Recording and Microphone
2. one-click `Open System Settings` per missing permission
3. explicit `Re-check` path that reruns preflight diagnostics
4. screen-permission restart advisory when Screen Recording settings are opened

## Implementation

New file: `app/AppShell/PermissionRemediationViewModel.swift`

### Models

1. `RemediablePermission`
- `screen_recording`
- `microphone`

2. `PermissionReadiness`
- `granted`
- `missing`

3. `PermissionRemediationItem`
- mapped permission
- readiness status
- source check IDs
- detail text
- remediation text

### View-model behavior

1. Runs `recordit preflight --mode live --json` via `RecorditPreflightRunner`.
2. Maps check IDs:
- Screen Recording lane: `screen_capture_access` or `display_availability`
- Microphone lane: `microphone_access`
3. Marks a permission `missing` when any mapped check is `FAIL`.
4. Exposes `missingPermissions` for clear UI identification.
5. `openSettings(for:)` opens deep links:
- `Privacy_ScreenCapture`
- `Privacy_Microphone`
6. Opening Screen Recording settings sets:
- `shouldShowScreenRecordingRestartAdvisory = true`
7. `recheckPermissions()` reruns preflight and refreshes permission state.

## UX Policy Alignment

The restart advisory string matches the journey decision:

`You may need to quit and reopen Recordit after changing Screen Recording access.`

## Validation

`permission_remediation_smoke.swift` validates:

1. missing-permission detection (screen missing, microphone granted)
2. settings deep-link targets for screen and microphone
3. restart advisory trigger after opening screen settings
4. deterministic re-check refresh from missing -> granted
