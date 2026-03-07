# bd-3rqf — Graceful live stop handshake before interrupt/terminate fallback

Date: 2026-03-07
Related bead: `bd-3rqf`
Primary implementation sources:
- `app/RuntimeProcessLayer/ProcessBackedRuntimeService.swift`
- `src/live_capture.rs`
- `src/bin/transcribe_live/runtime_live_stream.rs`
Primary exercised coverage:
- `app/RuntimeProcessLayer/process_lifecycle_integration_smoke.swift`
- `app/ViewModels/runtime_stop_finalization_smoke.swift`
- `tests/live_stream_stop_marker_finalize_integration.rs`

## Purpose

Document the current graceful-stop contract for live sessions: the app asks the runtime to drain and finalize first, then escalates through interrupt and terminate fallback only when bounded stages expire.

## Canonical handshake

The current stop path is intentionally three-stage and emits explicit stop diagnostics.

### Stage 1: request graceful stop

For `RuntimeControlAction.stop`, `ProcessBackedRuntimeService` now:
- resolves the active session root for the runtime process
- writes `session.stop.request` into that session root
- waits a bounded grace interval for the runtime to stop naturally

The bounded grace interval is controlled by:
- `stopTimeoutSeconds`
- `gracefulStopTimeoutSeconds`
- `boundedGracefulStopTimeout()`

Current rule:
- total stop control is bounded by `stopTimeoutSeconds`
- graceful wait is clamped via `boundedGracefulStopTimeout()`
- remaining time is split across interrupt and terminate fallback windows

### Stage 2: interrupt fallback

If the graceful request does not settle in time (or the request marker cannot be written), control falls back to interrupt (`SIGINT`) with a bounded interrupt wait.

### Stage 3: terminate fallback

If interrupt fallback still does not settle, control escalates to terminate (`SIGTERM`) with a bounded terminate wait and final timeout cleanup.

## Stop diagnostics contract

Stop control now emits stage/timing metadata so downstream triage can identify where stop failed or succeeded:

- `stop_strategy` (`graceful_handshake`, `interrupt_fallback`, `terminate_fallback`, `terminate_timeout`)
- `graceful_request_written`
- `graceful_wait_ms`
- `interrupt_wait_ms`
- `terminate_wait_ms`
- `stop_timeout_seconds`
- `graceful_timeout_seconds`
- `interrupt_timeout_seconds`
- `terminate_timeout_seconds`
- `escalation_reason`

For successful stop control, metadata is attached to `RuntimeControlResult.detail`.
For timeout/error outcomes, metadata is attached to `AppServiceError.debugDetail`.

## Runtime-side contract

The live runtime already consumes the stop-request marker instead of requiring an immediate external interrupt.

Current marker path:
- `session.stop.request`

Current behavior on the runtime side:
- live capture and live-stream runtime code watch for the stop marker
- when the marker appears, the runtime transitions into a drain/finalization path rather than treating stop as an abrupt kill-only event
- marker-driven runs are expected to preserve final manifest and transcript artifacts when the runtime can finalize cleanly

## Lifecycle invariants now on disk

The current codebase already asserts these invariants.

### Process-layer smoke invariants

`app/RuntimeProcessLayer/process_lifecycle_integration_smoke.swift` covers:
- stale `session.stop.request` markers are cleared on launch so they do not poison the next run
- explicit stop writes a request that helper runtimes observe as `REQUEST`
- the request marker is removed after control settles
- marker-driven graceful stop can drive `RuntimeViewModel` finalization to `.completed`
- if graceful stop does not complete, stop falls back to interrupt behavior
- if interrupt fallback does not settle, stop escalates to terminate behavior
- stop detail/debug metadata records strategy + escalation reason + per-stage timing

### View-model bounded finalization invariants

`app/ViewModels/runtime_stop_finalization_smoke.swift` covers:
- successful stop can keep polling until a pending manifest becomes final
- missing manifest paths time out into explicit recovery states
- retry-stop and retry-finalize recovery flows preserve bounded behavior
- interruption contexts distinguish empty-session versus partial-artifact failure modes

### Rust end-to-end invariant

`tests/live_stream_stop_marker_finalize_integration.rs` covers:
- writing `session.stop.request` during a live-stream run drains and finalizes the same run
- the runtime settles within a bounded timeout after the marker is written
- finalized artifacts (`session.wav`, `session.jsonl`, `session.manifest.json`) still exist after marker-driven shutdown
- lifecycle phases preserve `active -> draining -> shutdown` ordering

## Truthful scope boundary

This bead establishes the graceful-stop handshake contract itself.

It does **not** by itself prove every downstream stop/finalization guarantee. In particular, downstream work still exists for:
- broader stop/finalization stress coverage
- richer artifact evidence around stop failures beyond process-control metadata
- additional unit/e2e protection over bounded wait policy and manifest outcomes

That is why `bd-p77p`, `bd-2fic`, and `bd-1qjo` remain separate downstream beads.

## Current contract summary

The truthful current stop contract is now:
- stop prefers a session-root handshake (`session.stop.request`) before forced fallback
- graceful/interrupt/terminate waits are bounded, not unbounded
- stale stop markers are cleaned up at launch and after settled control
- graceful stop can finalize into a completed session when the runtime cooperates
- interrupt and terminate fallback remain available for unresponsive runtimes
- diagnostics identify whether stop settled in graceful handshake, interrupt fallback, terminate fallback, or timed out

## Decision

`bd-3rqf` is satisfied by the implementation and exercised coverage already on disk:
- the stop-request marker path exists in the app process layer
- the runtime consumes that marker
- smoke and integration coverage assert both graceful success and forced-fallback behavior

Future stop/finalization beads should build on this handshake instead of reintroducing direct interrupt-first stop semantics.
