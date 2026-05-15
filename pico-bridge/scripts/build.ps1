# One-shot Windows build helper for the Pico firmware.
#
# Usage:
#   .\scripts\build.ps1                       # default: build both boards
#   .\scripts\build.ps1 -Board pico2_w        # only Pico 2 W (RP2350)
#   .\scripts\build.ps1 -Board pico_w         # only Pico W / WH (RP2040)
#   .\scripts\build.ps1 -SdkPath C:\pico-sdk  # use a local SDK checkout
#   .\scripts\build.ps1 -Clean                # wipe per-board build dirs first
#
# After a successful build:
#   dist\couchlink-pico2w.uf2   <- Pico 2 W image
#   dist\couchlink-picow.uf2    <- Pico W / Pico WH image
#
# `couchlink flash` (no args) finds whichever board is in BOOTSEL and
# picks the matching file automatically.

param(
    [ValidateSet("all", "pico2_w", "pico_w")]
    [string]$Board = "all",
    [string]$SdkPath = "",
    [switch]$Clean,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# Project root = parent of scripts/, regardless of caller cwd.
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$DistDir     = Join-Path $ProjectRoot "dist"

# Board -> per-board build dir + canonical output filename.
$BoardInfo = @{
    "pico2_w" = @{ BuildDir = "build-pico2w"; Dist = "couchlink-pico2w.uf2"; Label = "Pico 2 W (RP2350)" }
    "pico_w"  = @{ BuildDir = "build-picow";  Dist = "couchlink-picow.uf2";  Label = "Pico W / WH (RP2040)" }
}

$BoardsToBuild = if ($Board -eq "all") { @("pico2_w", "pico_w") } else { @($Board) }

# Toolchain pre-flight.
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
    Write-Host "  - winget install Arm.GnuArmEmbeddedToolchain"
    Write-Host "Then reopen PowerShell and rerun this script."
    exit 1
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

foreach ($b in $BoardsToBuild) {
    $info     = $BoardInfo[$b]
    $buildDir = Join-Path $ProjectRoot $info.BuildDir
    $distFile = Join-Path $DistDir $info.Dist

    Write-Host ""
    Write-Host "===== $($info.Label) =====" -ForegroundColor Cyan

    if ($Clean -and (Test-Path $buildDir)) {
        Write-Host "Removing $buildDir"
        Remove-Item -Recurse -Force $buildDir
    }
    New-Item -ItemType Directory -Force -Path $buildDir | Out-Null

    $ConfigureArgs = @(
        "-S", $ProjectRoot,
        "-B", $buildDir,
        "-G", "Ninja",
        "-DPICO_BOARD=$b"
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
        # Tell pico_sdk_import.cmake to fetch into a shared location next
        # to (not inside) the per-board build dir, so building both boards
        # only clones the SDK once.
        $ConfigureArgs += "-DPICO_SDK_FETCH_FROM_GIT=ON"
    }

    Write-Host "Configuring $b..."
    & cmake @ConfigureArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host "Building $b..."
    & cmake --build $buildDir --target pico_bridge -j
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $Uf2 = Join-Path $buildDir "pico_bridge.uf2"
    if (-not (Test-Path $Uf2)) {
        Write-Host "Build appeared to succeed but no UF2 was produced at $Uf2" -ForegroundColor Red
        exit 1
    }
    Copy-Item -Force -Path $Uf2 -Destination $distFile
    Write-Host "  -> $distFile" -ForegroundColor Green
}

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host "Release artifacts:"
foreach ($b in $BoardsToBuild) {
    $info = $BoardInfo[$b]
    Write-Host "  $(Join-Path $DistDir $info.Dist)"
}
