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

function Get-HeadCommit {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    $head = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "git rev-parse HEAD failed with exit code $LASTEXITCODE"
    }
    return $head.Trim()
}

$repo = Join-Path ([System.IO.Path]::GetTempPath()) ('changelog-test-' + [System.Guid]::NewGuid().ToString('N'))
$script = Join-Path $PSScriptRoot 'Update-Changelog.ps1'
$changelog = Join-Path $repo 'CHANGELOG.md'
$sample = Join-Path $repo 'sample.txt'

try {
    $null = New-Item -ItemType Directory -Path $repo -Force
    Push-Location -LiteralPath $repo

    Invoke-Git -Arguments @('init', '-q')
    Invoke-Git -Arguments @('config', 'user.email', 'dev@example.com')
    Invoke-Git -Arguments @('config', 'user.name', 'Dev')

    $seedChangelog = @(
        '# Changelog',
        '',
        '## Unreleased',
        '',
        '_No notable changes since the last release._'
    ) -join "`n"
    [IO.File]::WriteAllText($changelog, "$seedChangelog`r`n")

    Invoke-Git -Arguments @('add', '.')
    Invoke-Git -Arguments @('commit', '-m', 'chore: base')

    [IO.File]::WriteAllText($sample, "one`r`n")
    Invoke-Git -Arguments @('add', '.')
    Invoke-Git -Arguments @('commit', '-m', 'feat(cli): add setup menu (2026.6.4.0-ABCD)')
    $first = Get-HeadCommit

    [IO.File]::WriteAllText($sample, "two`r`n")
    Invoke-Git -Arguments @('add', '.')
    Invoke-Git -Arguments @('commit', '-m', 'docs: update readme')
    $second = Get-HeadCommit

    & $script -Mode Append -Range "$first^..$second" -RepoRoot $repo
    if ($LASTEXITCODE -ne 0) { throw 'Append failed.' }

    $text = [IO.File]::ReadAllText($changelog)
    if ($text -notmatch '### Added' -or $text -notmatch '\*\*cli:\*\* Add setup menu') {
        throw 'Append did not add the expected feature entry.'
    }
    if ($text -match 'update readme') {
        throw 'Append included a docs-only commit.'
    }

    $promote = @{
        Mode     = 'Promote'
        Version  = 'v2026.6.4.0'
        Repo     = 'owner/repo'
        RepoRoot = $repo
        NowUtc   = [datetime]::Parse('2026-06-16T01:30:00Z')
    }
    & $script @promote
    if ($LASTEXITCODE -ne 0) { throw 'Promote failed.' }

    $text = [IO.File]::ReadAllText($changelog)
    if ($text -notmatch '## \[v2026\.6\.4\.0\]') {
        throw 'Promote did not create a versioned section.'
    }
    if ($text -notmatch '-- 2026-06-15') {
        throw 'Promote should date releases by America/Chicago during UTC rollover.'
    }

    $notes = & $script -Mode Notes -ForVersion -Version 'v2026.6.4.0' -RepoRoot $repo
    if ($notes -notmatch 'Add setup menu') {
        throw 'Notes did not return the promoted section.'
    }

    Write-Host 'Update-Changelog tests passed.'
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $repo) {
        Remove-Item -LiteralPath $repo -Recurse -Force
    }
}

exit 0
