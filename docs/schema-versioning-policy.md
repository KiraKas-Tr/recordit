# Schema Versioning and Breaking-Change Policy (bd-3kd8)

Date: 2026-03-02  
Status: canonical policy for machine-readable contract evolution in Phase D+

## 1. Scope

This policy governs versioning and compatibility for machine-readable `recordit` contracts, including:

- `contracts/runtime-jsonl.schema.vN.json`
- `contracts/session-manifest.schema.vN.json`
- `contracts/recordit-cli-contract.vN.json`
- `contracts/runtime-mode-matrix.vN.json`
- `contracts/recordit-exit-code-contract.vN.json`

It also governs what `recordit inspect-contract` is allowed to expose for each symbolic contract name.

## 2. Version Markers and Artifact Naming

All contract artifacts must be explicitly versioned in both filename and payload metadata.

Required conventions:

- filename suffix uses `.vN` where `N` is an integer major version (`v1`, `v2`, ...)
- schema payloads should expose version in `$id` and/or title
- non-schema contract JSON should include a `schema_version` or equivalent explicit version field

Examples:

- `contracts/runtime-jsonl.schema.v1.json`
- `contracts/session-manifest.schema.v1.json`
- `contracts/recordit-exit-code-contract.v1.json`

## 3. Additive vs Breaking Rules

Changes are **additive** (no major version bump required) only when all of the following hold:

- existing required fields are unchanged
- existing field meaning is unchanged
- enums are only expanded (never narrowed)
- new fields are optional (or gated to preserve old valid payloads)
- symbolic contract names in `inspect-contract` are unchanged

Changes are **breaking** (major version bump required) if any of the following occur:

- required field removed, renamed, or type-changed
- optional field becomes required
- enum value removed or semantic reinterpretation of an existing value
- structural shape changes that invalidate previously valid payloads
- exit-code class semantics change for an existing exit code
- symbolic contract name removal or incompatible command-surface contract drift

## 4. Mandatory Process for Breaking Changes

When a breaking change is introduced, maintainers must do all of the following in one tracked effort:

1. Create/attach a bead explicitly labeled as migration/versioning work.
2. Publish a new major contract artifact (`vN+1`) instead of mutating `vN` in place.
3. Keep prior major artifact(s) available in repository history and documented migration notes.
4. Update `recordit inspect-contract` behavior and docs to describe version transition behavior.
5. Add or update tests/CI checks that validate both new contract shape and migration expectations.
6. Add explicit operator/agent migration notes that state:
   - what changed
   - why it is breaking
   - how automation should migrate
   - cutover expectations/timeline

## 5. Inspect-Contract Compatibility Expectations

`recordit inspect-contract` must remain stable for symbolic discovery names. For each symbolic name:

- the command must return machine-readable output on stdout
- payload must be deterministic and parseable by CI/agents
- if a contract is temporarily unavailable, the response must still be machine-readable and explicit

When migrating major versions, symbolic discovery may continue to return the current canonical version, but migration notes must point to the exact versioned artifact names and compatibility impact.

## 6. Practical Checklist

Before merging any contract-affecting change, confirm:

- additive vs breaking classification is explicit
- version bump decision is documented
- artifact filename/version markers are correct
- inspect-contract output remains machine-readable
- migration notes exist when required
- contract tests pass in CI (`make contracts-ci`)
