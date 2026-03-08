# bd-2jsx — Startup runtime binary readiness and remediation

Implemented startup runtime binary readiness checks so missing/non-executable binaries are surfaced before runtime start flows.

## What Changed

1. Added startup readiness contracts in `RuntimeProcessManager`:
- `RuntimeBinaryReadinessStatus`
- `RuntimeBinaryReadinessCheck`
- `RuntimeBinaryReadinessReport`
- `RuntimeBinaryResolver.startupReadinessReport()`

2. Added `RuntimeBinaryReadinessService`:
- `RuntimeBinaryReadinessChecking` protocol
- `evaluateStartupReadiness()` for deterministic startup checks
- `startupBlockingError(from:)` mapping readiness failures to `AppServiceError(.runtimeUnavailable)` with remediation/debug detail

3. Hardened override-path validation:
- `RuntimeBinaryResolver` now validates override absolute-ness using `NSString.isAbsolutePath` before URL normalization.
- Prevents relative overrides from being silently treated as missing absolute paths.

4. Wired readiness into app-shell startup/onboarding gate:
- `AppShellViewModel` now stores `startupRuntimeReadinessReport` and `startupRuntimeReadinessFailure`.
- Returning users (`firstRun == false`) with failing readiness are routed to recovery immediately.
- `completeOnboardingIfReady(...)` now refreshes and enforces runtime readiness before allowing onboarding completion.

5. Added smoke coverage:
- `app/RuntimeProcessLayer/runtime_binary_readiness_smoke.swift`
  - ready PATH binaries
  - invalid relative override
  - non-executable override
  - startup recovery routing + onboarding gate failure mapping
- Updated `app/AppShell/onboarding_completion_smoke.swift` to inject deterministic ready runtime-readiness checker.

## Validation

- Module compile:
```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Accessibility/AccessibilityContracts.swift \
  app/Navigation/NavigationModels.swift \
  app/Navigation/AppNavigationCoordinator.swift \
  app/Preflight/PreflightRunner.swift \
  app/Preflight/PreflightGatingPolicy.swift \
  app/AppShell/PreflightViewModel.swift \
  app/AppShell/ModelSetupViewModel.swift \
  app/AppShell/OnboardingCompletionStore.swift \
  app/RuntimeProcessLayer/RuntimeProcessManager.swift \
  app/RuntimeProcessLayer/RuntimeBinaryReadinessService.swift \
  app/AppShell/AppShellViewModel.swift \
  -module-name RecordItRuntimeReadiness \
  -o /tmp/RecordItRuntimeReadiness.swiftmodule
```

- Smokes:
```bash
swiftc ... app/RuntimeProcessLayer/runtime_binary_readiness_smoke.swift -o /tmp/runtime_binary_readiness_smoke && /tmp/runtime_binary_readiness_smoke
# runtime_binary_readiness_smoke: PASS

swiftc ... app/AppShell/onboarding_completion_smoke.swift -o /tmp/onboarding_completion_smoke && /tmp/onboarding_completion_smoke
# onboarding_completion_smoke: PASS
```

- UBS (scoped):
```bash
UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/RuntimeProcessLayer/RuntimeProcessManager.swift \
  app/RuntimeProcessLayer/RuntimeBinaryReadinessService.swift \
  app/AppShell/AppShellViewModel.swift \
  app/AppShell/onboarding_completion_smoke.swift \
  app/RuntimeProcessLayer/runtime_binary_readiness_smoke.swift
```

Result: `0 critical / 0 warning`.
