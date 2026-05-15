param(
    # Release tags pass the bare version with no leading v.
    [string]$Version = "",

    # Produce dist/ParsecToDreamcast-v<version>.zip and a manifest TSV.
    [switch]$Package,

    # Skip firmware build and reuse dist/pico-build/pico_bridge.uf2.
    [switch]$SkipPico,

    # Skip host build and reuse bridge/target/release/ptd-bridge.exe.
    [switch]$SkipBridge,

    # Optional artifact root. Defaults to dist/.
    [string]$ArtifactsDir = ""
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$RepoRoot = (Resolve-Path $PSScriptRoot).Path
$DistRoot = if ($ArtifactsDir) {
    [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $ArtifactsDir))
} else {
    Join-Path $RepoRoot "dist"
}
$StageDir = Join-Path $DistRoot "ParsecToDreamcast"
$StateFile = Join-Path $RepoRoot ".local_build_state.json"

if ($Version) {
    if ($Version -notmatch '^\d{4}\.\d+\.\d+\.\d+(-[A-Fa-f0-9]{4})?$') {
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

Write-Host "Build version: $FullVersion" -ForegroundColor Magenta

New-Item -ItemType Directory -Force -Path $DistRoot | Out-Null
if (Test-Path -LiteralPath $StageDir) {
    Remove-Item -LiteralPath $StageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

$BridgeExe = Join-Path $RepoRoot "bridge\target\release\ptd-bridge.exe"
if (-not $SkipBridge) {
    Write-Host ""
    Write-Host "Building Windows bridge..." -ForegroundColor Cyan
    cargo build --manifest-path (Join-Path $RepoRoot "bridge\Cargo.toml") --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
if (-not (Test-Path -LiteralPath $BridgeExe)) {
    throw "Missing bridge executable at $BridgeExe"
}
Copy-Item -LiteralPath $BridgeExe -Destination (Join-Path $StageDir "ptd-bridge.exe") -Force

$PicoBuildDir = "..\dist\pico-build"
$FirmwareSource = Join-Path $RepoRoot "dist\pico-build\pico_bridge.uf2"
if (-not $SkipPico) {
    Write-Host ""
    Write-Host "Building Pico firmware..." -ForegroundColor Cyan
    & (Join-Path $RepoRoot "pico-bridge\scripts\build.ps1") -Release -BuildDir $PicoBuildDir
    if ($LASTEXITCODE -ne 0) { throw "Pico firmware build failed" }
}
if (-not (Test-Path -LiteralPath $FirmwareSource)) {
    throw "Missing firmware at $FirmwareSource"
}
Copy-Item -LiteralPath $FirmwareSource -Destination (Join-Path $StageDir "pico-bridge.uf2") -Force

Copy-Item -LiteralPath (Join-Path $RepoRoot "setup.ps1") -Destination (Join-Path $StageDir "setup.ps1") -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot "LICENSE") -Destination (Join-Path $StageDir "LICENSE") -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot "NOTICE") -Destination (Join-Path $StageDir "NOTICE") -Force

$ReleaseReadme = @"
ParsecToDreamcast $FullVersion

1. Extract the full zip.
2. Open PowerShell in this folder.
3. Run:
   powershell -ExecutionPolicy Bypass -File .\setup.ps1

The setup script flashes the Pico, provisions Wi-Fi, checks discovery, and can
add ptd-bridge.exe to Windows startup.
"@
Set-Content -LiteralPath (Join-Path $StageDir "README.txt") -Value $ReleaseReadme -Encoding ASCII

Write-Host ""
Write-Host "Staged release folder: $StageDir" -ForegroundColor Green

if ($Package) {
    $ZipPath = Join-Path $DistRoot "ParsecToDreamcast-v$FullVersion.zip"
    $ManifestPath = Join-Path $DistRoot "ParsecToDreamcast-v$FullVersion.manifest.tsv"
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
