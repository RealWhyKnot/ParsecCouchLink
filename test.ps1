# couchlink test wrapper
#
# Runs one named diagnostic check in isolation. Useful when doctor
# flagged a single failure and you want to iterate against just that
# check while debugging.
#
# Usage:
#   .\test.ps1 <name>
#
# Names:
#   xinput        XInput driver round-trip
#   paths         config/log directory permissions
#   firewall      Windows Firewall rule for couchlink.exe
#   startup       Windows Startup shortcut presence
#   discover      LAN discovery against a known Pico
#   cdc           USB-CDC port openable in setup mode
#   ack-identity  full ack/identity round-trip over UDP

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\data\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "test-$Stamp.log"
try { Start-Transcript -Path $TranscriptPath -IncludeInvocationHeader | Out-Null } catch {}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    try { Stop-Transcript | Out-Null } catch {}
    exit 1
}

if ($args.Count -eq 0) {
    Write-Host "Usage: .\test.ps1 <name>" -ForegroundColor Yellow
    Write-Host "Names: xinput, paths, firewall, startup, discover, cdc, ack-identity"
    try { Stop-Transcript | Out-Null } catch {}
    exit 2
}

& $BridgeExe test @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
