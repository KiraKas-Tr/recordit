@echo off
cd /d "%~dp0"
echo Starting recordit live transcription (English)...
echo Speak into your microphone. Press Ctrl+C to stop early.
echo.
target\release\recordit.exe run --mode live --duration-sec 60 --model artifacts\bench\models\whispercpp\ggml-tiny.en.bin
echo.
echo Session saved. Press any key to exit.
pause >nul
