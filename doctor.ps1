# couchlink doctor wrapper
#
# Runs every diagnostic check (paths, XInput, Windows Startup shortcut,
# firewall, Wi-Fi band, CDC, LAN discovery) and reports PASS / WARN /
# FAIL / SKIP with hints. Exit codes: 0 clean, 1 warnings only,
# 2 hard fail, 3 setup incomplete.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "doctor-$Stamp.log"
try { Start-Transcript -Path $TranscriptPath -IncludeInvocationHeader | Out-Null } catch {}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    try { Stop-Transcript | Out-Null } catch {}
    exit 1
}

& $BridgeExe doctor @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
