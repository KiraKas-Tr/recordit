# bd-5cz8 — Test Surface Inventory and Realism Catalog
Date: 2026-03-06
Related bead: `bd-5cz8`  
Parent feature: `bd-2elo`  
Downstream dependents: `bd-eq01`, `bd-39jy`
## Purpose
Create the exhaustive file-level catalog that sits underneath the critical-path coverage docs. This inventory answers a narrower but foundational question: **what executable test or verification surfaces exist today, where do they live, and what realism seams do they depend on?**
This catalog is intentionally broader than `docs/bd-39i6-critical-surface-coverage-matrix.md`. That earlier matrix is critical-path scoped. This file is the exhaustive surface register used to support downstream gap/policy work.
## Method
- scanned Rust inline `#[test]` files under `src/`
- scanned external Rust test crates under `tests/*.rs`
- scanned Swift standalone smoke executables under `app/**/*_smoke.swift`
- scanned XCTest/XCUITest suites under `app/RecorditAppTests` and `app/RecorditAppUITests`
- scanned Python verification tests under `tests/**/test_*.py`
- scanned primary shell/Python verification harnesses under `scripts/`
- classified realism conservatively from explicit markers such as `Mock*`, `Stub*`, `fixture`, `RECORDIT_UI_TEST_MODE`, `RECORDIT_FAKE_CAPTURE_FIXTURE`, temp-directory setup, and packaged-app/signing operations
- when a harness was obviously seam-bound by category/role even without an inline marker token, emitted a conservative fallback seam reason so downstream census work would not undercount it
## Snapshot
- total inventoried surfaces: **86**
- primary surfaces: **85**
- supporting harnesses: **1**
- category counts: rust inline **15**, rust external **26**, Swift smokes **29**, XCTest **1**, XCUITest **1**, Python tests **3**, shell harnesses **10**, Python harnesses **1**
- layer counts: unit **15**, integration **23**, scripted-e2e **8**, smoke **29**, XCTest **1**, XCUITest **1**, contract-test/harness **4**, release-script **5**
- realism counts: mock **18**, fixture **40**, temp-filesystem **9**, packaged-app **5**, live-real **14**
## High-Signal Findings
1. **The Swift app-layer test surface is still heavily seam-driven.** Many app, preflight, onboarding, export, and session-list surfaces rely on `Mock*`, `Stub*`, preview DI, or scripted manifests instead of the production app environment.
2. **XCUITest is real app launch, but not production-real behavior.** `app/RecorditAppUITests/RecorditAppUITests.swift` launches the app while explicitly enabling `RECORDIT_UI_TEST_MODE`, runtime overrides, and scripted runtime/preflight scenarios.
3. **Rust has broad coverage, but many “integration” lanes are still fixture-backed.** External tests such as replay, contract, fault-injection, and compatibility lanes often depend on frozen inputs or fake-capture seams.
4. **The strongest packaged/release verification lanes live in shell harnesses.** `gate_packaged_live_smoke.sh`, `verify_recordit_release_context.sh`, `gate_v1_acceptance.sh`, and `ci_recordit_xctest_evidence.sh` are the current retained-evidence backbone for packaged validation.
5. **Support/build scripts are not being overstated as proof.** `scripts/create_recordit_dmg.sh` is included as a supporting release harness, not as equivalent proof that DMG install/open is already fully verified.
## Known Realism Seams To Track Downstream
- `RECORDIT_UI_TEST_MODE` and scripted UI-test runtime behavior
- `Mock*` / `Stub*` service injection in Swift app-level tests and smokes
- fixture/replay/frozen WAV/JSON/model inputs in Rust, Swift, and shell lanes
- `RECORDIT_FAKE_CAPTURE_FIXTURE` and other deterministic capture substitutions
- temp-directory/process orchestration that proves logic but not full packaged-app behavior
## Existing Docs This Inventory Feeds
- `docs/bd-39i6-critical-surface-coverage-matrix.md`
- `docs/bd-39i6-critical-path-test-realism-inventory.md`
- `docs/bd-39i6-canonical-downstream-matrix.md`
- `docs/bd-11vg-critical-surface-gap-report.md`
## Exclusions / Boundaries
- helper-only summary scripts such as `scripts/*_summary.py` and support libraries such as `scripts/e2e_evidence_lib.sh` were not counted as primary test surfaces
- this inventory is file-level, not per-test-case certification; downstream beads can fan out rows if they need test-case granularity
- realism is intentionally conservative: if a file mixes real process/file I/O with fixtures or scripted dependencies, the row is tagged by the strongest seam rather than the strongest marketing claim
## Output
Machine-readable companion: `docs/bd-5cz8-test-surface-inventory.csv`
