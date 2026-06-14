# couchlink lab wrapper
#
# Runs unattended Pico hardware bench scenarios and records a transcript.

$ErrorActionPreference = "Stop"

$Root      = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "couchlink.exe"

$LogDir = Join-Path $env:LOCALAPPDATA "ParsecCouchLink\data\logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$TranscriptPath = Join-Path $LogDir "lab-$Stamp.log"
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

& $BridgeExe lab @args
$ExitCode = $LASTEXITCODE
try { Stop-Transcript | Out-Null } catch {}
exit $ExitCode
