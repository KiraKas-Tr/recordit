# Migration Note: Legacy `--live-chunked` Naming

This project now distinguishes taxonomy intent from runtime artifact labels:

- `--live-stream` selects true concurrent capture + transcription while recording.
- `--live-chunked` selects the representative-chunked validation path (captured WAV + near-live scheduler semantics).

`--live-chunked` is intentionally retained for compatibility in v1 so existing replay/gate tooling remains stable while migration work completes.

## Current Contract (v1)

| Taxonomy mode | Selector | `runtime_mode` artifact value | Intent |
|---|---|---|---|
| `representative-offline` | `<default>` | `representative-offline` | deterministic offline artifact validation |
| `representative-chunked` | `--live-chunked` | `live-chunked` | near-live scheduler validation on captured audio |
| `live-stream` | `--live-stream` | `live-stream` | true concurrent capture + transcription |

## Operator Guidance

- Use `--live-stream` when you want true live behavior during recording.
- Use `--live-chunked` when you want deterministic representative/near-live validation.
- Prefer `runtime_mode_taxonomy` for interpretation logic in tools and dashboards.
- Treat `runtime_mode=live-chunked` as a compatibility label, not proof of true live-stream behavior.

## Deprecation Path

1. Keep `--live-chunked` in v1 for compatibility while gate tooling/reporting is stabilized.
2. Use v1 acceptance-gate evidence to confirm migration safety for downstream operators/tooling.
3. Run explicit go/no-go review before any selector deprecation/removal.

See also:
- `docs/realtime-contracts.md`
- `docs/architecture.md`
- `docs/gate-phase-next-report.md`
