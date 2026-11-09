#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch] $DoctorOnly,
    [switch] $SkipIntro
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Write-Section {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Text)

    Write-Host ''
    Write-Host $Text -ForegroundColor Cyan
}

function Write-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Message)

    Write-Host ''
    Write-Host $Message -ForegroundColor Red
}

function Start-SetupTranscript {
    [CmdletBinding()]
    [OutputType([bool])]
    param([string] $Path)

    try {
        $null = Start-Transcript -Path $Path -IncludeInvocationHeader
        return $true
    } catch [System.InvalidOperationException], [System.NotSupportedException], [System.IO.IOException] {
        Write-Host "(transcript could not start: $($_.Exception.Message))" -ForegroundColor DarkYellow
    } catch [System.UnauthorizedAccessException], [System.ArgumentException] {
        Write-Host "(transcript could not start: $($_.Exception.Message))" -ForegroundColor DarkYellow
    }
    return $false
}

function Stop-SetupTranscript {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool] $Started)

    if ($Started) {
        $null = Stop-Transcript
    }
}

$LogDir = Join-Path $env:LOCALAPPDATA 'ParsecCouchLink\data\logs'
$null = New-Item -ItemType Directory -Force -Path $LogDir
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$TranscriptPath = Join-Path $LogDir "setup-$Stamp.log"
$transcribing = Start-SetupTranscript -Path $TranscriptPath

$BridgeExe = Join-Path $PSScriptRoot 'couchlink.exe'
$Pico2wUf2 = Join-Path $PSScriptRoot 'couchlink-pico2w.uf2'
$PicowUf2 = Join-Path $PSScriptRoot 'couchlink-picow.uf2'
$LegacyUf2 = Join-Path $PSScriptRoot 'couchlink-pico.uf2'

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    $message = @(
        'couchlink.exe was not found next to setup.ps1.',
        'Extract the release zip first, or run pico-bridge\scripts\build.ps1',
        'and bridge\cargo build --release from source.'
    ) -join ' '
    Write-Failure -Message $message
    Stop-SetupTranscript -Started $transcribing
    exit 1
}

if ($DoctorOnly) {
    & $BridgeExe doctor
    exit $LASTEXITCODE
}

$HavePico2w = Test-Path -LiteralPath $Pico2wUf2
$HavePicow = Test-Path -LiteralPath $PicowUf2
$HaveLegacy = Test-Path -LiteralPath $LegacyUf2

if (-not ($HavePico2w -or $HavePicow -or $HaveLegacy)) {
    $message = @(
        'No firmware file found next to setup.ps1.',
        'Expected at least one of: couchlink-pico2w.uf2, couchlink-picow.uf2.',
        'Download the full release zip, not just the script.'
    ) -join ' '
    Write-Failure -Message $message
    Stop-SetupTranscript -Started $transcribing
    exit 1
}

if (-not $SkipIntro) {
    $summary = @(
        'This will flash your Pico, put your Wi-Fi on it, test that the PC can find it,',
        'and offer to add the bridge to Windows startup.'
    ) -join ' '
    $bootselStep = @(
        '1. Hold BOOTSEL, plug the Pico in, then release as soon as Windows shows',
        'a removable drive (RPI-RP2 or RP2350).'
    ) -join ' '

    Write-Host 'Parsec CouchLink setup' -ForegroundColor Green
    Write-Host ''
    Write-Host $summary
    Write-Host 'You do not need a controller for setup; controller routing is tested when you start streaming.'
    Write-Host ''
    Write-Host 'Have these ready:'
    Write-Host '  - Windows 10/11 PC running Parsec'
    Write-Host '  - Raspberry Pi Pico 2 W or Pico W (or Pico WH)'
    Write-Host '  - Micro-USB data cable'
    Write-Host '  - 2.4 GHz Wi-Fi name and password'
    Write-Host '  - USB4MAPLE or another USB-to-console adapter'
    Write-Host ''
    Write-Host 'Available firmware in this folder:'
    if ($HavePico2w) { Write-Host '  - couchlink-pico2w.uf2  (for Pico 2 W / RP2350)' }
    if ($HavePicow) { Write-Host '  - couchlink-picow.uf2   (for Pico W / WH / RP2040)' }
    if ($HaveLegacy -and -not $HavePico2w) {
        Write-Host '  - couchlink-pico.uf2    (legacy name; used as the Pico 2 W image)'
    }
    Write-Host ''
    Write-Host 'The Wi-Fi password is sent to the Pico over USB setup mode. It is not saved on this PC.'

    Write-Section -Text 'What will happen'
    Write-Host $bootselStep
    Write-Host '2. The bridge detects which Pico you have and copies the matching firmware.'
    Write-Host '3. The Pico reboots as a USB serial setup device. (Do not press BOOTSEL during this reboot.)'
    Write-Host '4. You enter your 2.4 GHz Wi-Fi credentials.'
    Write-Host '5. The Pico joins Wi-Fi and the bridge checks discovery.'
    Write-Host '6. The bridge can add a Windows Startup shortcut.'

    Write-Host ''
    Read-Host 'Press Enter when you are ready'
}

Write-Section -Text 'Starting bridge setup'
& $BridgeExe setup
$ExitCode = $LASTEXITCODE

if ($ExitCode -eq 0) {
    $done = @(
        'Leave the Pico plugged into your console adapter.',
        'Have the remote player join through Parsec,',
        'then run couchlink.exe or reboot if you accepted the startup shortcut.'
    ) -join ' '
    Write-Section -Text 'Done'
    Write-Host $done
    Stop-SetupTranscript -Started $transcribing
    exit 0
}

Write-Section -Text 'Setup did not finish'
Write-Host ''
Write-Host "If you'd like help, here's everything we'd want to see:"
Write-Host "  1. This transcript:  $TranscriptPath"
Write-Host "  2. Bridge logs:      $LogDir"
Write-Host '  3. A bundle ZIP:     .\couchlink.exe bundle'
Write-Host '     (the bundle includes logs, doctor output, and -- if the Pico is'
Write-Host '      still in setup mode -- firmware diagnostics. No Wi-Fi password.)'
Write-Host ''
Write-Host 'Open an issue with the bundle attached:'
Write-Host '  https://github.com/RealWhyKnot/ParsecCouchLink/issues' -ForegroundColor Cyan
Write-Host ''
Write-Host 'For a quick local self-check first, run:'
Write-Host '  powershell -ExecutionPolicy Bypass -File .\setup.ps1 -DoctorOnly'
Stop-SetupTranscript -Started $transcribing
exit $ExitCode
