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
    [string]$Version = "",
    [switch]$Clean,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# Project root = parent of scripts/, regardless of caller cwd.
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$RepoRoot    = Split-Path -Parent $ProjectRoot
$DistDir     = Join-Path $ProjectRoot "dist"
$StateFile   = Join-Path $RepoRoot ".local_build_state.json"
$VersionFile = Join-Path $RepoRoot "version.txt"

function Enable-RepoGitHooks {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) { return }
    if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot ".githooks"))) { return }

    Push-Location $RepoRoot
    try {
        $currentHooksPath = & git config --get core.hooksPath 2>$null
        if ($LASTEXITCODE -ne 0) { $currentHooksPath = "" }
        if ($currentHooksPath -ne ".githooks") {
            & git config core.hooksPath ".githooks"
            if ($LASTEXITCODE -eq 0) {
                Write-Host "Activated .githooks/ via core.hooksPath"
            }
        }
    } finally {
        Pop-Location
    }
}

function Write-VersionStamp {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Version
    )

    $encoding = New-Object System.Text.UTF8Encoding -ArgumentList $false
    [System.IO.File]::WriteAllText($Path, $Version, $encoding)
}

Enable-RepoGitHooks

# Board -> per-board build dir + canonical output filename.
$BoardInfo = @{
    "pico2_w" = @{ BuildDir = "build-pico2w"; Dist = "couchlink-pico2w.uf2"; Label = "Pico 2 W (RP2350)" }
    "pico_w"  = @{ BuildDir = "build-picow";  Dist = "couchlink-picow.uf2";  Label = "Pico W / WH (RP2040)" }
}

$BoardsToBuild = if ($Board -eq "all") { @("pico2_w", "pico_w") } else { @($Board) }

if ($Version -ne "") {
    if ($Version -notmatch '^\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?$') {
        Write-Host "Invalid -Version '$Version'. Expected YYYY.M.D.N or YYYY.M.D.N-XXXX." -ForegroundColor Red
        exit 1
    }
    $FirmwareVersion = $Version
} else {
    $Today = Get-Date -Format "yyyy.M.d"
    $BuildCount = 0
    if (Test-Path -LiteralPath $StateFile) {
        $State = Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
        if ($State.Date -eq $Today) {
            $BuildCount = [int]$State.Count + 1
        }
    }
    $Suffix = [Guid]::NewGuid().ToString("N").Substring(0, 4).ToUpperInvariant()
    $FirmwareVersion = "$Today.$BuildCount-$Suffix"
    @{ Date = $Today; Count = $BuildCount } |
        ConvertTo-Json |
        Set-Content -LiteralPath $StateFile -Encoding UTF8
}

Write-VersionStamp -Path $VersionFile -Version $FirmwareVersion
Write-Host "Firmware version: $FirmwareVersion" -ForegroundColor Magenta

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

if ($Clean) {
    $SdkSubbuild = Join-Path $ProjectRoot "build-_pico_sdk\pico_sdk-subbuild"
    if (Test-Path -LiteralPath $SdkSubbuild) {
        Write-Host "Removing stale SDK subbuild $SdkSubbuild"
        Remove-Item -LiteralPath $SdkSubbuild -Recurse -Force
    }
}

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
        "-DPICO_BOARD=$b",
        "-DPICO_BRIDGE_FW_VERSION=$FirmwareVersion"
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
