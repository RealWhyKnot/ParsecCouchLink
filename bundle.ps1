# couchlink bundle wrapper
#
# Produces a ZIP of recent logs + diagnostics that's safe to attach to
# a bug report. Wi-Fi password and SSID are not included.
#
# The ZIP also tries to capture the Pico's in-RAM diag log over USB
# CDC. That capture only succeeds if the Pico is currently in setup
# mode and visible to Windows as a COM port -- in run mode it cannot
# be reached. If you're chasing a setup-mode bug, run this script
# while the Pico is still plugged in and the wizard is still on the
# stage that failed.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "bundle-$Stamp.log"
try { Start-Transcript -Path $TranscriptPath -IncludeInvocationHeader | Out-Null } catch {}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    try { Stop-Transcript | Out-Null } catch {}
    exit 1
}

& $BridgeExe bundle @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
