#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [ValidateSet('all', 'pico2_w', 'pico_w')]
    [string] $Board = 'all',
    [string] $SdkPath = '',
    [string] $Version = '',
    [switch] $Clean,
    [switch] $Release
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Enable-RepoGitHook {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory = $true)][string] $RepoRoot)

    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) { return }
    if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot '.githooks'))) { return }

    Push-Location -LiteralPath $RepoRoot
    try {
        $currentHooksPath = & { $ErrorActionPreference = 'Continue'; git config --get core.hooksPath 2>$null }
        if ($LASTEXITCODE -ne 0) { $currentHooksPath = '' }
        if ($currentHooksPath -ne '.githooks') {
            & git config core.hooksPath '.githooks'
            if ($LASTEXITCODE -eq 0) {
                Write-Host 'Activated .githooks/ via core.hooksPath'
            }
        }
    } finally {
        Pop-Location
    }
}

function Write-VersionStamp {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [string] $Version
    )

    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        $null = New-Item -ItemType Directory -Force -Path $parent
    }
    $encoding = New-Object System.Text.UTF8Encoding -ArgumentList $false
    [System.IO.File]::WriteAllText($Path, $Version, $encoding)
}

function Get-PrivateVersionFile {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory = $true)][string] $RepoRoot)

    $git = Get-Command git -ErrorAction SilentlyContinue
    if ($git) {
        Push-Location -LiteralPath $RepoRoot
        try {
            $path = & {
                $ErrorActionPreference = 'Continue'
                git rev-parse --git-path couchlink-version.txt 2>$null
            }
            if ($LASTEXITCODE -eq 0 -and $path) {
                $path = $path.Trim()
                if ([System.IO.Path]::IsPathRooted($path)) {
                    return [System.IO.Path]::GetFullPath($path)
                }
                return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $path))
            }
        } finally {
            Pop-Location
        }
    }

    return (Join-Path (Join-Path $RepoRoot '.git') 'couchlink-version.txt')
}

function Remove-LegacyVersionFile {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory = $true)][string] $RepoRoot)

    $legacy = Join-Path $RepoRoot 'version.txt'
    if (Test-Path -LiteralPath $legacy) {
        Remove-Item -LiteralPath $legacy -Force
    }
}

function Get-LocalBuildVersion {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory = $true)][string] $StateFile)

    $today = Get-Date -Format 'yyyy.M.d'
    $buildCount = 0
    if (Test-Path -LiteralPath $StateFile) {
        $state = [IO.File]::ReadAllText($StateFile) | ConvertFrom-Json
        $dateProperty = $state.PSObject.Properties['Date']
        if ($null -ne $dateProperty -and $dateProperty.Value -eq $today) {
            $buildCount = [int]$state.Count + 1
        }
    }
    $suffix = [Guid]::NewGuid().ToString('N').Substring(0, 4).ToUpperInvariant()
    $stateJson = @{ Date = $today; Count = $buildCount } | ConvertTo-Json -Depth 2
    [IO.File]::WriteAllText($StateFile, $stateJson + [Environment]::NewLine)
    return "$today.$buildCount-$suffix"
}

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$RepoRoot = Split-Path -Parent $ProjectRoot
$DistDir = Join-Path $ProjectRoot 'dist'
$StateFile = Join-Path $RepoRoot '.local_build_state.json'

Enable-RepoGitHook -RepoRoot $RepoRoot
Remove-LegacyVersionFile -RepoRoot $RepoRoot
$VersionFile = Get-PrivateVersionFile -RepoRoot $RepoRoot

$BoardInfo = @{
    'pico2_w' = @{ BuildDir = 'build-pico2w'; Dist = 'couchlink-pico2w.uf2'; Label = 'Pico 2 W (RP2350)' }
    'pico_w'  = @{ BuildDir = 'build-picow'; Dist = 'couchlink-picow.uf2'; Label = 'Pico W / WH (RP2040)' }
}

$BoardsToBuild = if ($Board -eq 'all') { @('pico2_w', 'pico_w') } else { @($Board) }

if ($Version -ne '') {
    if ($Version -notmatch '^\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?$') {
        Write-Host "Invalid -Version '$Version'. Expected YYYY.M.D.N or YYYY.M.D.N-XXXX." -ForegroundColor Red
        exit 1
    }
    $FirmwareVersion = $Version
} else {
    $FirmwareVersion = Get-LocalBuildVersion -StateFile $StateFile
}

Write-VersionStamp -Path $VersionFile -Version $FirmwareVersion
Write-Host "Firmware version: $FirmwareVersion" -ForegroundColor Magenta

$Missing = [System.Collections.Generic.List[string]]::new()
foreach ($tool in @('cmake', 'ninja', 'arm-none-eabi-gcc')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        $Missing.Add($tool)
    }
}
if ($Missing.Count -gt 0) {
    Write-Host "Missing toolchain components: $($Missing -join ', ')" -ForegroundColor Red
    Write-Host ''
    Write-Host 'Install with:'
    Write-Host '  - winget install Kitware.CMake'
    Write-Host '  - winget install Ninja-build.Ninja'
    Write-Host '  - winget install Arm.GnuArmEmbeddedToolchain'
    Write-Host 'Then reopen PowerShell and rerun this script.'
    exit 1
}

$null = New-Item -ItemType Directory -Force -Path $DistDir

if ($Clean) {
    $SdkSubbuild = Join-Path $ProjectRoot 'build-_pico_sdk\pico_sdk-subbuild'
    if (Test-Path -LiteralPath $SdkSubbuild) {
        Write-Host "Removing stale SDK subbuild $SdkSubbuild"
        Remove-Item -LiteralPath $SdkSubbuild -Recurse -Force
    }
}

foreach ($b in $BoardsToBuild) {
    $info = $BoardInfo[$b]
    $buildDir = Join-Path $ProjectRoot $info['BuildDir']
    $distFile = Join-Path $DistDir $info['Dist']

    Write-Host ''
    Write-Host "===== $($info['Label']) =====" -ForegroundColor Cyan

    if ($Clean -and (Test-Path -LiteralPath $buildDir)) {
        Write-Host "Removing $buildDir"
        Remove-Item -LiteralPath $buildDir -Recurse -Force
    }
    $null = New-Item -ItemType Directory -Force -Path $buildDir

    $buildType = if ($Release) { 'Release' } else { 'RelWithDebInfo' }
    $ConfigureArgs = [System.Collections.Generic.List[string]]::new()
    $ConfigureArgs.AddRange([string[]]@(
            '-S', $ProjectRoot,
            '-B', $buildDir,
            '-G', 'Ninja',
            "-DPICO_BOARD=$b",
            "-DPICO_BRIDGE_FW_VERSION=$FirmwareVersion",
            "-DCMAKE_BUILD_TYPE=$buildType"
        ))

    if ($SdkPath -ne '') {
        if (-not (Test-Path -LiteralPath $SdkPath)) {
            Write-Host "SDK path does not exist: $SdkPath" -ForegroundColor Red
            exit 1
        }
        $ConfigureArgs.Add("-DPICO_SDK_PATH=$SdkPath")
    } elseif ($env:PICO_SDK_PATH -and (Test-Path -LiteralPath $env:PICO_SDK_PATH)) {
        Write-Host "Using PICO_SDK_PATH from environment: $($env:PICO_SDK_PATH)"
    } else {
        $ConfigureArgs.Add('-DPICO_SDK_FETCH_FROM_GIT=ON')
    }

    Write-Host "Configuring $b..."
    & cmake @ConfigureArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host "Building $b..."
    & cmake --build $buildDir --target pico_bridge -j
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $Uf2 = Join-Path $buildDir 'pico_bridge.uf2'
    if (-not (Test-Path -LiteralPath $Uf2)) {
        Write-Host "Build appeared to succeed but no UF2 was produced at $Uf2" -ForegroundColor Red
        exit 1
    }
    Copy-Item -Force -LiteralPath $Uf2 -Destination $distFile
    Write-Host "  -> $distFile" -ForegroundColor Green
}

Write-Host ''
Write-Host 'Done.' -ForegroundColor Green
Write-Host 'Release artifacts:'
foreach ($b in $BoardsToBuild) {
    $info = $BoardInfo[$b]
    Write-Host "  $(Join-Path $DistDir $info['Dist'])"
}

exit 0
