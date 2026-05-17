# couchlink logs wrapper
#
# Prints the path of the active log file, or tails it live.
#
# Usage:
#   .\logs.ps1            # print log directory + active file path
#   .\logs.ps1 --tail     # follow the active log file as it grows
#
# Logs rotate; old files are kept under the same directory.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Write-Host "couchlink.exe was not found next to this script." -ForegroundColor Red
    Write-Host "Extract the full release zip into one folder, then run this from that folder."
    exit 1
}

& $BridgeExe logs @args
exit $LASTEXITCODE
