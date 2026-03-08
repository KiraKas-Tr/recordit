# bd-fggr: Interruption Recovery UX (Resume and Safe Finalize)

## Goal

Handle interruption/crash outcomes without silent data loss by exposing recoverable-state guidance and explicit recovery actions.

## Delivered

1. `app/ViewModels/RuntimeViewModel.swift`
2. `app/ViewModels/runtime_stop_finalization_smoke.swift`
3. `docs/bd-fggr-interruption-recovery.md`

## What Landed

1. Added interruption-specific recovery actions:
1. `resume_session`
2. `safe_finalize`

2. Added recoverable interruption context surface:
1. `InterruptionRecoveryClassification.recoverable_interruption`
2. `InterruptionRecoveryContext` with:
   - `sessionRoot`
   - plain-language `summary`
   - plain-language `guidance`
   - normalized action list

3. Exposed explicit runtime recovery commands:
1. `resumeInterruptedSession(explicitModelPath:)`
2. `safeFinalizeInterruptedSession()`

4. Updated failure classification mapping:
1. interruption/timeouts now include `safe_finalize` and interruption context
2. process-interruption failures include `resume_session` + `safe_finalize`
3. stop-control interruption branch now prepends interruption-aware actions while preserving existing fallback actions

5. Hardened accessibility metadata for recovery UX affordances:
1. added `resume_interrupted_session` and `safe_finalize_session` elements
2. updated deterministic focus plan ordering
3. added keyboard shortcuts for resume and safe finalize

## Interruption Branch Coverage

`runtime_stop_finalization_smoke` now validates:
1. timeout branch maps to safe-finalize-first recovery actions
2. failed manifest branch exposes resume/safe-finalize options
3. process interruption branch emits recoverable interruption context with preserved `sessionRoot`
4. `resumeInterruptedSession()` relaunches runtime in the interrupted root
5. `safeFinalizeInterruptedSession()` finalizes artifacts and clears interruption context

## Acceptance Mapping

1. Interruption states preserve partial artifacts:
   - recoverable context keeps `sessionRoot` after interruption failures.
2. Resume/Safe Finalize choices are presented:
   - explicit recovery actions + callable runtime methods.
3. Plain-language recovery guidance:
   - interruption context includes operator-facing summary/guidance copy.
4. Integration-style interruption branches covered:
   - expanded smoke includes interruption/resume/safe-finalize flow assertions.
