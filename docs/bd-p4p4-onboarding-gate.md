# bd-p4p4: Onboarding Completion Gate and Persistence

## Goal

Allow onboarding completion only when both model validation and preflight gating are ready, persist completion state across relaunches, and provide a reset path for recovery.

## Delivered

1. `app/AppShell/OnboardingCompletionStore.swift`
2. `app/AppShell/AppShellViewModel.swift`
3. `app/AppShell/onboarding_completion_smoke.swift`

## What Landed

1. Added persisted onboarding completion store abstraction:
- protocol: `OnboardingCompletionStore`
- default implementation: `UserDefaultsOnboardingCompletionStore`
- key-backed completion state (`recordit.onboarding.completed`)

2. Updated `AppShellViewModel` initialization routing semantics:
- if `firstRun` override is provided, behavior remains explicit
- otherwise, initial root now derives from persisted completion state (`onboarding` vs `main_runtime`)

3. Added explicit onboarding completion gate API:
- `completeOnboardingIfReady(modelSetup:preflight:)`
- requires BOTH:
  - `ModelSetupViewModel.canStartLiveTranscribe == true`
  - `PreflightViewModel.canProceedToLiveTranscribe == true`
- on success:
  - persists completion
  - clears gate failure
  - dispatches `.finishOnboarding`
- on failure:
  - preserves onboarding root
  - sets `onboardingGateFailure` with deterministic code:
    - `.modelUnavailable` when model setup invalid
    - `.preflightFailed` when preflight not passable

4. Added recovery/reset path:
- `resetOnboardingCompletion()` clears persisted completion and routes back to onboarding

## Acceptance Mapping

1. Completion only after required model + preflight readiness:
- enforced by `completeOnboardingIfReady(...)` dual-gate checks.

2. Persisted state restores across relaunch:
- initialization reads completion store when `firstRun` override is absent.

3. Reset path available for recovery:
- explicit `resetOnboardingCompletion()` implementation.

## Validation

`onboarding_completion_smoke.swift` verifies:
1. fresh launch routes to onboarding
2. successful gate persists completion and routes to main runtime
3. relaunch with same store restores main runtime root
4. reset clears completion and returns to onboarding
5. model-invalid and preflight-not-ready attempts fail with expected error codes
