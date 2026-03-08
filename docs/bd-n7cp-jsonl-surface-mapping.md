# bd-n7cp JSONL Event Mapping for Transcript and Health Surfaces

## Scope

Implemented deterministic mapping of runtime JSONL events into:
- primary transcript surface rows (stable rows only)
- diagnostics/health surface signals (queue/trust/lifecycle/control)

Unknown/future event types are ignored safely.

## Implementation

### `app/Services/JsonlEventSurfaceMapper.swift`

Added `JsonlEventSurfaceMapper` with typed outputs:
- `RuntimeTranscriptSurfaceLine`
- `RuntimeDiagnosticSurfaceSignal`
- `RuntimeEventSurfaceSnapshot`
- `RuntimeDiagnosticCategory`

Mapping behavior:
1. Transcript surface:
   - includes only stable event types: `final`, `llm_final`, `reconciled_final`
   - ignores `partial`
   - deterministic ordering key (`startMs`, `endMs`, event rank, channel, segment IDs, text)
   - de-duplicates equivalent rows
   - applies reconciled preference so `reconciled_final` can suppress replaced `final` rows via `source_final_segment_id`
2. Diagnostics surface:
   - includes non-primary signals by type prefix:
     - `queue*` -> `.queue`
     - `trust*` -> `.trust`
     - `lifecycle*` -> `.lifecycle`
     - `control*` -> `.control`
3. Unknown/future event types:
   - ignored (neither transcript nor diagnostics)

### `app/Services/jsonl_event_surface_smoke.swift`

Added smoke coverage for:
- reconciled-vs-final replacement behavior
- stable transcript ordering
- diagnostics category mapping for queue/trust/lifecycle/control
- unknown event ignore safety

## Validation

```bash
swiftc -parse-as-library -emit-module \
  app/Services/ServiceInterfaces.swift \
  app/Services/JsonlEventSurfaceMapper.swift \
  -module-name RecordItJsonlEventSurface \
  -o /tmp/RecordItJsonlEventSurface.swiftmodule

swiftc \
  app/Services/ServiceInterfaces.swift \
  app/Services/JsonlEventSurfaceMapper.swift \
  app/Services/jsonl_event_surface_smoke.swift \
  -o /tmp/jsonl_event_surface_smoke && /tmp/jsonl_event_surface_smoke

UBS_MAX_DIR_SIZE_MB=5000 ubs \
  app/Services/JsonlEventSurfaceMapper.swift \
  app/Services/jsonl_event_surface_smoke.swift \
  docs/bd-n7cp-jsonl-surface-mapping.md
```
