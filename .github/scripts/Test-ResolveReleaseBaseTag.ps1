#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Git {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    $null = & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Add-TestCommit {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)]
        [string] $Repo,
        [Parameter(Mandatory = $true)]
        [string] $Content,
        [Parameter(Mandatory = $true)]
        [string] $Subject
    )

    [IO.File]::WriteAllText((Join-Path $Repo 'sample.txt'), $Content)
    Invoke-Git -Arguments @('add', 'sample.txt')
    Invoke-Git -Arguments @('commit', '-m', $Subject)
}

$resolver = Join-Path $PSScriptRoot 'Resolve-ReleaseBaseTag.ps1'
$repo = Join-Path ([System.IO.Path]::GetTempPath()) ('release-base-test-' + [System.Guid]::NewGuid().ToString('N'))

try {
    $null = New-Item -ItemType Directory -Path $repo -Force
    Push-Location -LiteralPath $repo

    Invoke-Git -Arguments @('init', '-q')
    Invoke-Git -Arguments @('config', 'user.email', 'release-test@example.com')
    Invoke-Git -Arguments @('config', 'user.name', 'Release Test')
    Invoke-Git -Arguments @('config', 'commit.gpgsign', 'false')
    Invoke-Git -Arguments @('config', 'tag.gpgsign', 'false')

    Add-TestCommit -Repo $repo -Content 'base' -Subject 'chore: base'
    Invoke-Git -Arguments @('tag', 'v2026.5.1.0')

    Add-TestCommit -Repo $repo -Content 'pre' -Subject 'feat: prerelease patch'
    Invoke-Git -Arguments @('tag', 'v2026.5.2.0-beta')

    Add-TestCommit -Repo $repo -Content 'stable' -Subject 'fix: stable patch'
    Invoke-Git -Arguments @('tag', 'v2026.5.3.0')

    $stableBase = & $resolver -Tag 'v2026.5.3.0'
    if ($stableBase -ne 'v2026.5.1.0') {
        throw "Stable base was '$stableBase', expected 'v2026.5.1.0'."
    }

    $preBase = & $resolver -Tag 'v2026.5.2.0-beta'
    if ($preBase -ne 'v2026.5.1.0') {
        throw "Prerelease base was '$preBase', expected 'v2026.5.1.0'."
    }

    $firstBase = & $resolver -Tag 'v2026.5.1.0'
    if ($firstBase) {
        throw "First-release base was '$firstBase', expected empty."
    }
    if ($LASTEXITCODE -ne 0) {
        throw "First-release resolve left LASTEXITCODE=$LASTEXITCODE, expected 0."
    }

    Write-Host 'Resolve-ReleaseBaseTag tests passed.'
} finally {
    Pop-Location
    Remove-Item -LiteralPath $repo -Recurse -Force
}

exit 0
