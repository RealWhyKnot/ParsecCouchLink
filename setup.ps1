param(
    [switch]$DoctorOnly,
    [switch]$SkipIntro
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$BridgeExe = Join-Path $Root "ptd-bridge.exe"
$Firmware = Join-Path $Root "pico-bridge.uf2"

function Write-Section {
    param([string]$Text)
    Write-Host ""
    Write-Host $Text -ForegroundColor Cyan
}

function Stop-Setup {
    param([string]$Message)
    Write-Host ""
    Write-Host $Message -ForegroundColor Red
    exit 1
}

if (-not (Test-Path -LiteralPath $BridgeExe)) {
    Stop-Setup "ptd-bridge.exe was not found next to setup.ps1. Extract the release zip first, or run build.ps1 from source."
}

if ($DoctorOnly) {
    & $BridgeExe doctor
    exit $LASTEXITCODE
}

if (-not (Test-Path -LiteralPath $Firmware)) {
    Stop-Setup "pico-bridge.uf2 was not found next to setup.ps1. Download the full release zip, not just the script."
}

if (-not $SkipIntro) {
    Write-Host "ParsecToDreamcast setup" -ForegroundColor Green
    Write-Host ""
    Write-Host "This will flash your Pico, put your Wi-Fi on it, test that the PC can find it, and offer to add the bridge to Windows startup."
    Write-Host ""
    Write-Host "Have these ready:"
    Write-Host "  - Windows 10/11 PC running Parsec"
    Write-Host "  - Raspberry Pi Pico 2 W"
    Write-Host "  - Micro-USB data cable"
    Write-Host "  - 2.4 GHz Wi-Fi name and password"
    Write-Host "  - USB4MAPLE or another USB-to-console adapter"
    Write-Host ""
    Write-Host "The Wi-Fi password is sent to the Pico over USB setup mode. It is not saved on this PC."

    Write-Section "What will happen"
    Write-Host "1. Hold BOOTSEL while plugging the Pico into this PC."
    Write-Host "2. The script copies pico-bridge.uf2 onto the Pico."
    Write-Host "3. The Pico reboots as a USB serial setup device."
    Write-Host "4. You enter your 2.4 GHz Wi-Fi credentials."
    Write-Host "5. The Pico joins Wi-Fi and the bridge checks discovery."
    Write-Host "6. The bridge can add a Windows Startup shortcut."

    Write-Host ""
    Read-Host "Press Enter when you are ready"
}

Write-Section "Starting bridge setup"
& $BridgeExe setup --uf2 $Firmware
$ExitCode = $LASTEXITCODE

if ($ExitCode -eq 0) {
    Write-Section "Done"
    Write-Host "Leave the Pico plugged into your console adapter. Have the remote player join through Parsec, then run ptd-bridge.exe or reboot if you accepted the startup shortcut."
    exit 0
}

Write-Section "Setup did not finish"
Write-Host "Run this for a health check after fixing the issue:"
Write-Host "  powershell -ExecutionPolicy Bypass -File .\setup.ps1 -DoctorOnly"
exit $ExitCode
