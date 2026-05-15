# One-shot Windows build helper for the Pico firmware.
#
# Usage:
#   .\scripts\build.ps1                       # default: pico2_w, fetch SDK
#   .\scripts\build.ps1 -Board pico_w         # original RP2040 + Wi-Fi
#   .\scripts\build.ps1 -SdkPath C:\pico-sdk  # use a local SDK checkout
#   .\scripts\build.ps1 -Clean                # wipe build dir first
#
# After a successful build:
#   build\pico_bridge.uf2  <- drag this onto a BOOTSEL Pico, or pass it
#                              to `ptd-bridge flash --uf2`.

param(
    [string]$Board = "pico2_w",
    [string]$SdkPath = "",
    [string]$BuildDir = "build",
    [switch]$Clean,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# Resolve the project root (parent of scripts/) regardless of cwd.
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$BuildPath   = Join-Path $ProjectRoot $BuildDir

if ($Clean -and (Test-Path $BuildPath)) {
    Write-Host "Removing $BuildPath"
    Remove-Item -Recurse -Force $BuildPath
}
New-Item -ItemType Directory -Force -Path $BuildPath | Out-Null

# Check toolchain prerequisites.
$Missing = @()
foreach ($tool in @("cmake", "ninja", "arm-none-eabi-gcc")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        $Missing += $tool
    }
}
if ($Missing.Count -gt 0) {
    Write-Host "Missing toolchain components: $($Missing -join ', ')" -ForegroundColor Red
    Write-Host ""
    Write-Host "Install with:"
    Write-Host "  - winget install Kitware.CMake"
    Write-Host "  - winget install Ninja-build.Ninja"
    Write-Host "  - winget install ARM.GnuArmEmbeddedToolchain"
    Write-Host "Then re-open PowerShell and rerun this script."
    exit 1
}

$ConfigureArgs = @(
    "-S", $ProjectRoot,
    "-B", $BuildPath,
    "-G", "Ninja",
    "-DPICO_BOARD=$Board"
)
if ($Release) {
    $ConfigureArgs += "-DCMAKE_BUILD_TYPE=Release"
} else {
    $ConfigureArgs += "-DCMAKE_BUILD_TYPE=RelWithDebInfo"
}

if ($SdkPath -ne "") {
    if (-not (Test-Path $SdkPath)) {
        Write-Host "SDK path does not exist: $SdkPath" -ForegroundColor Red
        exit 1
    }
    $ConfigureArgs += "-DPICO_SDK_PATH=$SdkPath"
} elseif ($env:PICO_SDK_PATH -and (Test-Path $env:PICO_SDK_PATH)) {
    Write-Host "Using PICO_SDK_PATH from environment: $($env:PICO_SDK_PATH)"
} else {
    Write-Host "No PICO_SDK_PATH set; CMake will fetch the SDK into the build dir."
    $ConfigureArgs += "-DPICO_SDK_FETCH_FROM_GIT=ON"
}

Write-Host "Configuring..."
& cmake @ConfigureArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Building..."
& cmake --build $BuildPath --target pico_bridge -j
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Uf2 = Join-Path $BuildPath "pico_bridge.uf2"
if (Test-Path $Uf2) {
    Write-Host ""
    Write-Host "Build OK: $Uf2" -ForegroundColor Green
} else {
    Write-Host "Build appeared to succeed but no UF2 was produced at $Uf2" -ForegroundColor Red
    exit 1
}
