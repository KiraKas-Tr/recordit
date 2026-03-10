# Operator Quickstart (`bd-1zru`)

Date: 2026-03-05  
Status: canonical GUI-first quickstart for `Recordit.app`

## Goal

Take a non-terminal operator from install to a validated first live session through the windowed `Recordit.app` path.

## Prerequisites

- macOS 15+
- Xcode command line tools
- local model assets available (`make setup-whispercpp-model`)

## GUI-First Path (Default)

### 1. Build the packaged app and DMG

```bash
make create-recordit-dmg RECORDIT_DMG_NAME=Recordit-local.dmg RECORDIT_DMG_VOLNAME='Recordit'
```

Expected output:
- `dist/Recordit.app`
- `dist/Recordit-local.dmg`

### 2. Install from DMG (drag-to-Applications)

1. Open `dist/Recordit-local.dmg`.
2. Confirm the mounted view contains:
   - `Recordit.app`
   - `Applications` alias/symlink.
3. Drag `Recordit.app` into `Applications`.

Optional terminal verification:

```bash
MOUNT_POINT="$(mktemp -d /tmp/recordit-dmg-XXXX)"
hdiutil attach dist/Recordit-local.dmg -nobrowse -readonly -mountpoint "$MOUNT_POINT"
ls -la "$MOUNT_POINT"
readlink "$MOUNT_POINT/Applications"
hdiutil detach "$MOUNT_POINT"
```

### 3. Launch `Recordit.app` and complete first-run onboarding

1. Launch `Recordit.app` from `Applications`.
2. On first run, complete onboarding in order:
   - Permission checks (Screen Recording + Microphone)
   - Model setup/readiness
   - Ready/complete step
3. If permission prompts do not appear automatically, grant access in System Settings and retry preflight from onboarding.

Validation target:
- onboarding transitions to main runtime screen only after required gates are green.

### 4. Run and validate first live session

1. In main runtime view, choose `Live Transcribe`.
2. Click `Start`.
3. Verify runtime status transitions to running and transcript/status activity appears.
4. Click `Stop`.
5. Verify summary/recovery UI appears with manifest-backed final status and artifact actions.

### 5. Validate artifacts

Confirm latest session contains:
- `session.input.wav`
- `session.wav`
- `session.jsonl`
- `session.manifest.json`

If using terminal checks, inspect the latest session under `artifacts/sessions/<date>/<timestamp>-live/`.

## Fallback Diagnostics (Non-Default)

The paths below are for engineering/support diagnostics only and are not the primary user journey:

- `make run-transcribe-app ...` (compatibility `SequoiaTranscribe.app` lane)
- `cargo run --bin recordit -- ...` direct CLI flows
- legacy `transcribe-live` debug flows

Policy references:
- `docs/adr-005-recordit-default-entrypoint.md`
- `docs/bd-14y4-sequoiatranscribe-fallback-policy.md`

When documenting user guidance, always present `Recordit.app` as default and label fallback lanes explicitly as compatibility/diagnostic only.


---

## Windows CLI Lane

> **Platform boundary:** macOS is the primary platform with a full GUI (`Recordit.app`).
> Windows supports the CLI tools only (`recordit.exe`, `transcribe-live.exe`).

### Prerequisites

- Windows 10 22H2+ or Windows 11 (x64)
- Rust stable + MSVC target:
  ```powershell
  rustup target add x86_64-pc-windows-msvc
  ```
- Visual Studio Build Tools ("Desktop development with C++" workload)
- Microphone and audio output device present and enabled

### Build

```powershell
cargo build --release --target x86_64-pc-windows-msvc --bin recordit --bin transcribe-live
```

### Preflight

```powershell
.\target\x86_64-pc-windows-msvc\release\transcribe-live.exe preflight
```

All three of `screen_capture_access`, `microphone_access`, and `backend_runtime` should be `pass` or `warn` (no `fail`) before starting a live session.

### Run

```powershell
.\target\x86_64-pc-windows-msvc\release\recordit.exe run --mode live --duration-sec 30 --out-wav session.wav --out-jsonl session.jsonl
```

### CI / Release artifact

The GitHub Actions workflow `.github/workflows/windows-cli-ci.yml` produces a zip artifact on every push to `main`:

```
recordit-windows-cli-<sha>.zip
  bin\
    recordit.exe
    transcribe-live.exe
```

Place backend helper binaries (e.g. `whisper-cli.exe`) alongside `transcribe-live.exe` or in the `bin\` directory to enable transcription.

### Support boundary

Windows CLI is fully supported. Windows GUI (`Recordit.app` equivalent) is **not** in scope for this project. For GUI usage, use macOS.
