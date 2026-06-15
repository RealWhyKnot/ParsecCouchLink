#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$repo = Join-Path ([System.IO.Path]::GetTempPath()) ("commit-hook-test-" + [System.Guid]::NewGuid().ToString("N"))
$pushedLocation = $false

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)

    & git @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Resolve-RepoGitPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $repo $Path))
}

try {
    New-Item -ItemType Directory -Path $repo -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot ".githooks") -Destination (Join-Path $repo ".githooks") -Recurse

    Push-Location $repo
    $pushedLocation = $true
    Invoke-Git @("init", "-q")
    Invoke-Git @("config", "user.email", "dev@example.com")
    Invoke-Git @("config", "user.name", "Dev")
    Invoke-Git @("config", "core.hooksPath", ".githooks")

    $today = Get-Date -Format "yyyy.M.d"
    $expectedVersion = "$today.42-ABCD"
    $privateVersionPath = Resolve-RepoGitPath ((& git rev-parse --git-path couchlink-version.txt).Trim())
    $privateParent = Split-Path -Parent $privateVersionPath
    New-Item -ItemType Directory -Path $privateParent -Force | Out-Null
    Set-Content -LiteralPath $privateVersionPath -Value $expectedVersion -Encoding ASCII

    Set-Content -LiteralPath "sample.txt" -Value "one" -Encoding ASCII
    Invoke-Git @("add", "sample.txt")
    Invoke-Git @("commit", "-m", "docs: sample")

    $subject = (& git log -1 --format=%s).Trim()
    $expectedSubject = "docs: sample ($expectedVersion)"
    if ($subject -ne $expectedSubject) {
        throw "prepare-commit-msg stamped '$subject', expected '$expectedSubject'."
    }

    if (Test-Path -LiteralPath (Join-Path $repo "version.txt")) {
        throw "prepare-commit-msg created root version.txt."
    }

    if (-not (Test-Path -LiteralPath $privateVersionPath)) {
        throw "prepare-commit-msg did not keep the private version stamp."
    }

    $prePushPath = Join-Path $repo ".githooks\pre-push"
    if (-not (Test-Path -LiteralPath $prePushPath)) {
        throw "Missing .githooks/pre-push."
    }

    $prePushText = Get-Content -LiteralPath $prePushPath -Raw
    if ($prePushText -notmatch "tools/lint\.ps1" -or $prePushText -notmatch "-SkipRust" -or $prePushText -notmatch "-SkipPowerShell") {
        throw "pre-push hook must run the firmware formatting lint path."
    }

    Write-Host "Commit hook tests passed."
} finally {
    if ($pushedLocation) {
        Pop-Location
    }
    if (Test-Path -LiteralPath $repo) {
        Remove-Item -LiteralPath $repo -Recurse -Force
    }
}
