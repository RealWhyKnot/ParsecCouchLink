# couchlink flash wrapper
#
# Detects a Pico in BOOTSEL mode and copies the matching firmware UF2
# onto it (couchlink-pico2w.uf2 for the RP2350 board, couchlink-picow.uf2
# for the RP2040 board). Use this when you need to re-flash without
# running the full setup wizard.
#
# Tip: this only flashes firmware. It does NOT push Wi-Fi credentials.
# After a re-flash, use configure-wifi.ps1 to send Wi-Fi to the Pico.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\data\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "flash-$Stamp.log"
try {
    Start-Transcript -Path $TranscriptPath -IncludeInvocationHeader | Out-Null
} catch {
    Write-Host "(transcript could not start: $($_.Exception.Message))" -ForegroundColor DarkYellow
}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    try { Stop-Transcript | Out-Null } catch {}
    exit 1
}

& $BridgeExe flash @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
