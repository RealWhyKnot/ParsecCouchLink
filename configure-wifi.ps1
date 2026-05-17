# couchlink configure-wifi wrapper
#
# Sends Wi-Fi SSID + password to a Pico that is currently in setup
# mode (USB CDC). Use this when only the Wi-Fi credentials need to
# change -- the router was replaced, the password rotated, etc.
#
# The Pico must already be running our firmware AND be in setup mode.
# If the Pico is in run mode (XInput) instead, follow the credential-
# wipe path in the wiki under "Recovery" first to drop it back into
# setup mode.
#
# The password is sent over USB and is not written to disk on this PC.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\data\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "configure-wifi-$Stamp.log"
try { Start-Transcript -Path $TranscriptPath -IncludeInvocationHeader | Out-Null } catch {}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    try { Stop-Transcript | Out-Null } catch {}
    exit 1
}

& $BridgeExe configure-wifi @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
