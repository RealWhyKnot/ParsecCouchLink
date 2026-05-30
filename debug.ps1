# couchlink debug wrapper
#
# Opens the Pico debug and recovery menu. Pass arguments through to use
# direct actions such as --status, --to-usb-debug, --to-wifi, or --logs.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\data\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "debug-$Stamp.log"
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

& $BridgeExe debug @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
