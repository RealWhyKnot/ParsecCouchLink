#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Git {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory = $true)][string[]] $Arguments)

    $null = & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Resolve-RepoGitPath {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory = $true)][string] $Repo,
        [Parameter(Mandatory = $true)][string] $Path
    )

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $Repo $Path))
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$repo = Join-Path ([System.IO.Path]::GetTempPath()) ('commit-hook-test-' + [System.Guid]::NewGuid().ToString('N'))
$pushedLocation = $false

try {
    $null = New-Item -ItemType Directory -Path $repo -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot '.githooks') -Destination (Join-Path $repo '.githooks') -Recurse

    Push-Location -LiteralPath $repo
    $pushedLocation = $true
    Invoke-Git -Arguments @('init', '-q')
    Invoke-Git -Arguments @('config', 'user.email', 'dev@example.com')
    Invoke-Git -Arguments @('config', 'user.name', 'Dev')
    Invoke-Git -Arguments @('config', 'core.hooksPath', '.githooks')

    $today = Get-Date -Format 'yyyy.M.d'
    $expectedVersion = "$today.42-ABCD"
    $gitPath = & git rev-parse --git-path couchlink-version.txt
    if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed with exit code $LASTEXITCODE" }
    $privateVersionPath = Resolve-RepoGitPath -Repo $repo -Path $gitPath.Trim()
    $privateParent = Split-Path -Parent $privateVersionPath
    $null = New-Item -ItemType Directory -Path $privateParent -Force
    [IO.File]::WriteAllText($privateVersionPath, "$expectedVersion`r`n")

    [IO.File]::WriteAllText((Join-Path $repo 'sample.txt'), "one`r`n")
    Invoke-Git -Arguments @('add', 'sample.txt')
    Invoke-Git -Arguments @('commit', '-m', 'docs: sample')

    $subject = & git log -1 --format=%s
    if ($LASTEXITCODE -ne 0) { throw "git log failed with exit code $LASTEXITCODE" }
    $subject = $subject.Trim()
    $expectedSubject = "docs: sample ($expectedVersion)"
    if ($subject -ne $expectedSubject) {
        throw "prepare-commit-msg stamped '$subject', expected '$expectedSubject'."
    }

    if (Test-Path -LiteralPath (Join-Path $repo 'version.txt')) {
        throw 'prepare-commit-msg created root version.txt.'
    }

    if (-not (Test-Path -LiteralPath $privateVersionPath)) {
        throw 'prepare-commit-msg did not keep the private version stamp.'
    }

    $prePushPath = Join-Path $repo '.githooks\pre-push'
    if (-not (Test-Path -LiteralPath $prePushPath)) {
        throw 'Missing .githooks/pre-push.'
    }

    $prePushText = [IO.File]::ReadAllText($prePushPath)
    if ($prePushText -notmatch '\.github/lint/lint\.ps1') {
        throw 'pre-push hook must run .github/lint/lint.ps1.'
    }
    if ($prePushText -match '-Skip|SKIP') {
        throw 'pre-push hook skips part of .github/lint/lint.ps1.'
    }

    Write-Host 'Commit hook tests passed.'
} finally {
    if ($pushedLocation) {
        Pop-Location
    }
    if (Test-Path -LiteralPath $repo) {
        Remove-Item -LiteralPath $repo -Recurse -Force
    }
}

exit 0
