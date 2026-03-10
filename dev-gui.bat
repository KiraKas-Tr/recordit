@echo off
setlocal

REM Ensure Rust/Cargo is in PATH
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

cd /d "%~dp0gui"

echo ============================================
echo   Recordit GUI - Windows Native Dev Mode
echo ============================================
echo.

REM node_modules from WSL contain Linux binaries — force reinstall on Windows
if exist "node_modules\.package-lock.json" (
    REM Quick check: if the tauri CLI binary is not a Windows exe, wipe and reinstall
    if not exist "node_modules\.bin\tauri.cmd" (
        echo [1/3] Reinstalling dependencies for Windows...
        rmdir /s /q node_modules 2>nul
        call npm install
    ) else (
        echo [1/3] Frontend dependencies OK
    )
) else (
    echo [1/3] Installing frontend dependencies...
    call npm install
)

REM Always rebuild recordit.exe in dev so GUI uses the latest CLI flags/behavior
cd /d "%~dp0"
echo [2/3] Building recordit.exe (debug)...
cargo build --bin recordit
if errorlevel 1 (
    echo ERROR: cargo build failed
    exit /b 1
)

REM Check for whisper-cli.exe
set WHISPER_CLI=
if exist "target\debug\whisper-cli.exe" set WHISPER_CLI=target\debug\whisper-cli.exe
if exist "target\release\whisper-cli.exe" set WHISPER_CLI=target\release\whisper-cli.exe
if "%WHISPER_CLI%"=="" (
    echo.
    echo WARNING: whisper-cli.exe not found in target\debug\ or target\release\
    echo          Transcription will fail without it.
    echo.
)

echo [3/3] Starting Tauri dev server...
echo.
cd /d "%~dp0gui"
call npm run tauri dev
