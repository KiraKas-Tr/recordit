param(
    [int]$Duration = 60,
    [string]$Model = "artifacts\bench\models\whispercpp\ggml-tiny.en.bin",
    [float]$VadThreshold = 0.02
)

Set-Location $PSScriptRoot

Write-Host ""
Write-Host "=== Recordit Live Transcription ==" -ForegroundColor Cyan
Write-Host "  Duration  : $Duration seconds (0 = unlimited)" -ForegroundColor Gray
Write-Host "  Model     : $Model" -ForegroundColor Gray
Write-Host "  VAD       : $VadThreshold" -ForegroundColor Gray
Write-Host ""
Write-Host ">> Speak into your microphone. Press Ctrl+C to stop early." -ForegroundColor Yellow
Write-Host ""

$args = @(
    "run", "--mode", "live",
    "--duration-sec", $Duration,
    "--model", $Model
)

& ".\target\release\recordit.exe" @args
