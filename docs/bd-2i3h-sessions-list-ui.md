# bd-2i3h Sessions List View Model

Implemented sessions-list UX state handling in `app/ViewModels/SessionListViewModel.swift`.

## Delivered Behavior

1. Filter controls:
   - mode: `all`, `live`, `record_only`
   - status: `all`, `pending`, `ok`, `degraded`, `failed`
   - free-text search
2. View-state model:
   - `idle`
   - `loading(previousItems:)`
   - `loaded(items:)`
   - `empty(title/detail)` (user-friendly copy)
   - `failed(error, recoverableItems:)` (non-blocking fallback)
3. Deterministic ordering preserved in the view model:
   - `startedAt` descending
   - `sessionID` ascending
   - `rootPath` ascending
4. Query mapping uses `SessionQuery(status:mode:searchText:)` with normalized search text.

## Acceptance Notes

- Supports mode/status filters, sort, and search.
- Empty/loading/error states are explicit and user-friendly.
- Failure path keeps last known items (`recoverableItems`) so sessions list remains usable.

## Validation

Smoke runner `app/ViewModels/session_list_smoke.swift` validates:

1. deterministic newest-first ordering
2. mode + status filtering behavior
3. unmatched search -> empty state
4. refresh failure fallback retains previously loaded sessions

