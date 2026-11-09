#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string] $Version = '',
    [switch] $Package,
    [switch] $SkipPico,
    [switch] $SkipBridge,
    [string] $ArtifactsDir = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Enable-RepoGitHook {
    [CmdletBinding()]
    [OutputType([void])]
    param()

    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) { return }
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot '.githooks'))) { return }

    $currentHooksPath = & { $ErrorActionPreference = 'Continue'; git config --get core.hooksPath 2>$null }
    if ($LASTEXITCODE -ne 0) { $currentHooksPath = '' }
    if ($currentHooksPath -ne '.githooks') {
        & git config core.hooksPath '.githooks'
        if ($LASTEXITCODE -eq 0) {
            Write-Host 'Activated .githooks/ via core.hooksPath'
        }
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

function Write-AsciiFile {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][string[]] $Lines
    )

    $newLine = [Environment]::NewLine
    [IO.File]::WriteAllText($Path, ($Lines -join $newLine) + $newLine, [System.Text.Encoding]::ASCII)
}

Set-Location -LiteralPath $PSScriptRoot

$RepoRoot = (Resolve-Path -LiteralPath $PSScriptRoot).Path
$DistRoot = if ($ArtifactsDir) {
    [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $ArtifactsDir))
} else {
    Join-Path $RepoRoot 'dist'
}
$StageDir = Join-Path $DistRoot 'ParsecCouchLink'
$StateFile = Join-Path $RepoRoot '.local_build_state.json'
$VersionFile = Get-PrivateVersionFile -RepoRoot $RepoRoot

Enable-RepoGitHook
Remove-LegacyVersionFile -RepoRoot $RepoRoot

if ($Version) {
    if ($Version -notmatch '^\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?$') {
        throw "Invalid -Version '$Version'. Expected YYYY.M.D.N or YYYY.M.D.N-XXXX."
    }
    $FullVersion = $Version
} else {
    $FullVersion = Get-LocalBuildVersion -StateFile $StateFile
}

Write-VersionStamp -Path $VersionFile -Version $FullVersion
Write-Host "Build version: $FullVersion" -ForegroundColor Magenta

$null = New-Item -ItemType Directory -Force -Path $DistRoot
if (Test-Path -LiteralPath $StageDir) {
    Remove-Item -LiteralPath $StageDir -Recurse -Force
}
$null = New-Item -ItemType Directory -Force -Path $StageDir

$BridgeExe = Join-Path $RepoRoot 'bridge\target\release\couchlink.exe'
if (-not $SkipBridge) {
    Write-Host ''
    Write-Host 'Building Windows bridge...' -ForegroundColor Cyan
    $PreviousBridgeVersion = $env:COUCHLINK_VERSION
    $env:COUCHLINK_VERSION = $FullVersion
    try {
        cargo build --manifest-path (Join-Path $RepoRoot 'bridge\Cargo.toml') --release
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    } finally {
        $env:COUCHLINK_VERSION = $PreviousBridgeVersion
    }
}
if (-not (Test-Path -LiteralPath $BridgeExe)) {
    throw "Missing bridge executable at $BridgeExe"
}
Copy-Item -LiteralPath $BridgeExe -Destination (Join-Path $StageDir 'couchlink.exe') -Force

$PicoDistDir = Join-Path $RepoRoot 'pico-bridge\dist'
$FirmwarePico2w = Join-Path $PicoDistDir 'couchlink-pico2w.uf2'
$FirmwarePicow = Join-Path $PicoDistDir 'couchlink-picow.uf2'
if (-not $SkipPico) {
    Write-Host ''
    Write-Host 'Building Pico firmware (both boards)...' -ForegroundColor Cyan
    & (Join-Path $RepoRoot 'pico-bridge\scripts\build.ps1') -Release -Version $FullVersion
    if ($LASTEXITCODE -ne 0) { throw 'Pico firmware build failed' }
}
if (-not (Test-Path -LiteralPath $FirmwarePico2w)) {
    throw "Missing firmware at $FirmwarePico2w (run without -SkipPico)"
}
if (-not (Test-Path -LiteralPath $FirmwarePicow)) {
    throw "Missing firmware at $FirmwarePicow (run without -SkipPico)"
}
Copy-Item -LiteralPath $FirmwarePico2w -Destination (Join-Path $StageDir 'couchlink-pico2w.uf2') -Force
Copy-Item -LiteralPath $FirmwarePicow -Destination (Join-Path $StageDir 'couchlink-picow.uf2') -Force

foreach ($script in @('setup.ps1')) {
    $src = Join-Path $RepoRoot $script
    if (-not (Test-Path -LiteralPath $src)) {
        throw "Missing release script: $src"
    }
    Copy-Item -LiteralPath $src -Destination (Join-Path $StageDir $script) -Force
}
Copy-Item -LiteralPath (Join-Path $RepoRoot 'LICENSE') -Destination (Join-Path $StageDir 'LICENSE') -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot 'NOTICE') -Destination (Join-Path $StageDir 'NOTICE') -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot 'CHANGELOG.md') -Destination (Join-Path $StageDir 'CHANGELOG.md') -Force

$ReleaseReadme = @"
Parsec CouchLink $FullVersion

First-time setup
----------------
1. Extract the full zip into one folder (avoid Program Files).
2. Open PowerShell in this folder.
3. Run:
   powershell -ExecutionPolicy Bypass -File .\setup.ps1

The setup script flashes the Pico, provisions Wi-Fi, checks discovery,
and can add couchlink.exe to Windows startup.

Daily use
---------
Run the app from this folder:

  couchlink.exe                  open the guided Basic/Advanced menu
  couchlink.exe run              start direct streaming
  couchlink.exe doctor           run every diagnostic check
  couchlink.exe bundle           produce a support-bundle ZIP for bug reports
  couchlink.exe logs --tail      follow the active log file
  couchlink.exe flash            re-flash without re-running setup
  couchlink.exe debug            Pico debug and recovery menu
  couchlink.exe configure-wifi   re-send Wi-Fi credentials
  couchlink.exe lab --scenario status
                                 development bench status snapshot

setup.ps1 writes a setup transcript to
  %LOCALAPPDATA%\ParsecCouchLink\data\logs
next to the bridge's own logs.
"@
Write-AsciiFile -Path (Join-Path $StageDir 'README.txt') -Lines @($ReleaseReadme)

Write-Host ''
Write-Host "Staged release folder: $StageDir" -ForegroundColor Green

if ($Package) {
    $ZipPath = Join-Path $DistRoot "ParsecCouchLink-v$FullVersion.zip"
    $ManifestPath = Join-Path $DistRoot "ParsecCouchLink-v$FullVersion.manifest.tsv"
    if (Test-Path -LiteralPath $ZipPath) { Remove-Item -LiteralPath $ZipPath -Force }
    if (Test-Path -LiteralPath $ManifestPath) { Remove-Item -LiteralPath $ManifestPath -Force }

    Compress-Archive -Path (Join-Path $StageDir '*') -DestinationPath $ZipPath -CompressionLevel Optimal

    $Rows = New-Object System.Collections.Generic.List[string]
    $Rows.Add("path`tbytes`tsha256")
    foreach ($item in (Get-ChildItem -LiteralPath $StageDir -Recurse -File | Sort-Object FullName)) {
        $Rel = $item.FullName.Substring($StageDir.Length).TrimStart('\', '/')
        $Hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToUpperInvariant()
        $Rows.Add("$Rel`t$($item.Length)`t$Hash")
    }
    Write-AsciiFile -Path $ManifestPath -Lines $Rows.ToArray()

    $Zip = Get-Item -LiteralPath $ZipPath
    Write-Host "Release zip:      $($Zip.FullName)" -ForegroundColor Green
    Write-Host "Release manifest: $ManifestPath" -ForegroundColor Green
}

exit 0
