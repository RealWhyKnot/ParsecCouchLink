param(
    # Release tags pass the bare version with no leading v.
    [string]$Version = "",

    # Produce dist/ParsecCouchLink-v<version>.zip and a manifest TSV.
    [switch]$Package,

    # Skip firmware build and reuse dist/pico-build/pico_bridge.uf2.
    [switch]$SkipPico,

    # Skip host build and reuse bridge/target/release/couchlink.exe.
    [switch]$SkipBridge,

    # Optional artifact root. Defaults to dist/.
    [string]$ArtifactsDir = ""
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Enable-RepoGitHooks {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) { return }
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot ".githooks"))) { return }

    $currentHooksPath = & git config --get core.hooksPath 2>$null
    if ($LASTEXITCODE -ne 0) { $currentHooksPath = "" }
    if ($currentHooksPath -ne ".githooks") {
        & git config core.hooksPath ".githooks"
        if ($LASTEXITCODE -eq 0) {
            Write-Host "Activated .githooks/ via core.hooksPath"
        }
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

$RepoRoot = (Resolve-Path $PSScriptRoot).Path
$DistRoot = if ($ArtifactsDir) {
    [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $ArtifactsDir))
} else {
    Join-Path $RepoRoot "dist"
}
$StageDir = Join-Path $DistRoot "ParsecCouchLink"
$StateFile = Join-Path $RepoRoot ".local_build_state.json"
$VersionFile = Join-Path $RepoRoot "version.txt"

Enable-RepoGitHooks

if ($Version) {
    if ($Version -notmatch '^\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?$') {
        throw "Invalid -Version '$Version'. Expected YYYY.M.D.N or YYYY.M.D.N-XXXX."
    }
    $FullVersion = $Version
} else {
    $Today = Get-Date -Format "yyyy.M.d"
    $BuildCount = 0
    if (Test-Path -LiteralPath $StateFile) {
        $State = Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
        if ($State.Date -eq $Today) { $BuildCount = [int]$State.Count + 1 }
    }
    $Suffix = [Guid]::NewGuid().ToString("N").Substring(0, 4).ToUpperInvariant()
    $FullVersion = "$Today.$BuildCount-$Suffix"
    @{ Date = $Today; Count = $BuildCount } |
        ConvertTo-Json |
        Set-Content -LiteralPath $StateFile -Encoding UTF8
}

Write-VersionStamp -Path $VersionFile -Version $FullVersion
Write-Host "Build version: $FullVersion" -ForegroundColor Magenta

New-Item -ItemType Directory -Force -Path $DistRoot | Out-Null
if (Test-Path -LiteralPath $StageDir) {
    Remove-Item -LiteralPath $StageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

$BridgeExe = Join-Path $RepoRoot "bridge\target\release\couchlink.exe"
if (-not $SkipBridge) {
    Write-Host ""
    Write-Host "Building Windows bridge..." -ForegroundColor Cyan
    cargo build --manifest-path (Join-Path $RepoRoot "bridge\Cargo.toml") --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
if (-not (Test-Path -LiteralPath $BridgeExe)) {
    throw "Missing bridge executable at $BridgeExe"
}
Copy-Item -LiteralPath $BridgeExe -Destination (Join-Path $StageDir "couchlink.exe") -Force

$PicoDistDir       = Join-Path $RepoRoot "pico-bridge\dist"
$FirmwarePico2w    = Join-Path $PicoDistDir "couchlink-pico2w.uf2"
$FirmwarePicow     = Join-Path $PicoDistDir "couchlink-picow.uf2"
if (-not $SkipPico) {
    Write-Host ""
    Write-Host "Building Pico firmware (both boards)..." -ForegroundColor Cyan
    & (Join-Path $RepoRoot "pico-bridge\scripts\build.ps1") -Release -Version $FullVersion
    if ($LASTEXITCODE -ne 0) { throw "Pico firmware build failed" }
}
if (-not (Test-Path -LiteralPath $FirmwarePico2w)) {
    throw "Missing firmware at $FirmwarePico2w (run without -SkipPico)"
}
if (-not (Test-Path -LiteralPath $FirmwarePicow)) {
    throw "Missing firmware at $FirmwarePicow (run without -SkipPico)"
}
Copy-Item -LiteralPath $FirmwarePico2w -Destination (Join-Path $StageDir "couchlink-pico2w.uf2") -Force
Copy-Item -LiteralPath $FirmwarePicow  -Destination (Join-Path $StageDir "couchlink-picow.uf2")  -Force

$ScriptFiles = @(
    "setup.ps1",
    "doctor.ps1",
    "bundle.ps1",
    "flash.ps1",
    "bootsel.ps1",
    "debug.ps1",
    "configure-wifi.ps1",
    "logs.ps1"
)
foreach ($script in $ScriptFiles) {
    $src = Join-Path $RepoRoot $script
    if (-not (Test-Path -LiteralPath $src)) {
        throw "Missing wrapper script: $src"
    }
    Copy-Item -LiteralPath $src -Destination (Join-Path $StageDir $script) -Force
}
Copy-Item -LiteralPath (Join-Path $RepoRoot "LICENSE") -Destination (Join-Path $StageDir "LICENSE") -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot "NOTICE") -Destination (Join-Path $StageDir "NOTICE") -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot "CHANGELOG.md") -Destination (Join-Path $StageDir "CHANGELOG.md") -Force

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
Each subcommand also has a one-shot wrapper script. Right-click and
"Run with PowerShell", or call from an existing PowerShell prompt:

  couchlink.exe          start the bridge for a Parsec session
  couchlink.exe test     run one diagnostic check by name
  doctor.ps1             run every diagnostic check
  bundle.ps1             produce a support-bundle ZIP for bug reports
  logs.ps1               print log path (use --tail to follow live)
  flash.ps1              re-flash without re-running setup
  bootsel.ps1            switch setup-mode USB Pico to BOOTSEL
  debug.ps1              Pico debug and recovery menu
  configure-wifi.ps1     re-send Wi-Fi credentials

The wrappers record a transcript under
  %LOCALAPPDATA%\ParsecCouchLink\data\logs
alongside the bridge's own logs, so one folder has everything a
bug report needs.
"@
Set-Content -LiteralPath (Join-Path $StageDir "README.txt") -Value $ReleaseReadme -Encoding ASCII

Write-Host ""
Write-Host "Staged release folder: $StageDir" -ForegroundColor Green

if ($Package) {
    $ZipPath = Join-Path $DistRoot "ParsecCouchLink-v$FullVersion.zip"
    $ManifestPath = Join-Path $DistRoot "ParsecCouchLink-v$FullVersion.manifest.tsv"
    if (Test-Path -LiteralPath $ZipPath) { Remove-Item -LiteralPath $ZipPath -Force }
    if (Test-Path -LiteralPath $ManifestPath) { Remove-Item -LiteralPath $ManifestPath -Force }

    Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -CompressionLevel Optimal

    $Rows = New-Object System.Collections.Generic.List[string]
    $Rows.Add("path`tbytes`tsha256")
    Get-ChildItem -LiteralPath $StageDir -Recurse -File |
        Sort-Object FullName |
        ForEach-Object {
            $Rel = $_.FullName.Substring($StageDir.Length).TrimStart('\', '/')
            $Hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToUpperInvariant()
            $Rows.Add("$Rel`t$($_.Length)`t$Hash")
        }
    Set-Content -LiteralPath $ManifestPath -Value $Rows -Encoding ASCII

    $Zip = Get-Item -LiteralPath $ZipPath
    Write-Host "Release zip:      $($Zip.FullName)" -ForegroundColor Green
    Write-Host "Release manifest: $ManifestPath" -ForegroundColor Green
}
