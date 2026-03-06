# bd-13kc: Legacy SequoiaTranscribe Path Compatibility Shim

## Scope

`bd-13kc` introduces a deterministic compatibility resolver for automation that still expects legacy `SequoiaTranscribe` executable paths.

Delivered:

- `scripts/resolve_sequoiatranscribe_compat.sh` (new)
- `scripts/gate_packaged_live_smoke.sh` updated to resolve executable via shim
- docs updates:
  - `docs/gate-packaged-live-smoke.md`
  - `docs/cli-entrypoint-compat-audit.md`

## Shim Contract

`scripts/resolve_sequoiatranscribe_compat.sh` resolution order:

1. `SEQUOIA_TRANSCRIBE_COMPAT_BIN` (if executable)
2. `dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe`

If no executable resolves:

- exits non-zero
- prints remediation (`make sign-transcribe` or set override path)

When resolved:

- prints compatibility warnings to stderr including a sunset target (`2026-Q3`)
- prints resolved executable path to stdout for machine consumption

## Regression Checks (Known Script Lane)

Executed checks:

```bash
bash -n scripts/resolve_sequoiatranscribe_compat.sh
bash -n scripts/gate_packaged_live_smoke.sh
make sign-transcribe SIGN_IDENTITY=-
scripts/resolve_sequoiatranscribe_compat.sh --root "$PWD"
```

Dry-run compatibility gate semantics:

```bash
make -n gate-packaged-live-smoke | rg 'resolve_sequoiatranscribe_compat.sh|gate_packaged_live_smoke.sh'
```

## Outcome

Known compatibility script path remains operational while path resolution is now centralized, warning-bearing, and sunset-oriented. This preserves legacy automation expectations without reintroducing SequoiaTranscribe as a default user-facing path.
