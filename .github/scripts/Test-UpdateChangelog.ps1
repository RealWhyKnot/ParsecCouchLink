#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$repo = Join-Path ([System.IO.Path]::GetTempPath()) ("changelog-test-" + [System.Guid]::NewGuid().ToString("N"))
$script = Join-Path $PSScriptRoot "Update-Changelog.ps1"

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)
    & git @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

try {
    New-Item -ItemType Directory -Path $repo -Force | Out-Null
    Push-Location $repo

    Invoke-Git @("init", "-q")
    Invoke-Git @("config", "user.email", "dev@example.com")
    Invoke-Git @("config", "user.name", "Dev")

    $seedChangelog = @(
        "# Changelog",
        "",
        "## Unreleased",
        "",
        "_No notable changes since the last release._"
    ) -join "`n"
    Set-Content -LiteralPath "CHANGELOG.md" -Value $seedChangelog -Encoding ASCII

    Invoke-Git @("add", ".")
    Invoke-Git @("commit", "-m", "chore: base")

    Set-Content -LiteralPath "sample.txt" -Value "one" -Encoding ASCII
    Invoke-Git @("add", ".")
    Invoke-Git @("commit", "-m", "feat(cli): add setup menu (2026.6.4.0-ABCD)")
    $first = (& git rev-parse HEAD).Trim()

    Set-Content -LiteralPath "sample.txt" -Value "two" -Encoding ASCII
    Invoke-Git @("add", ".")
    Invoke-Git @("commit", "-m", "docs: update readme")
    $second = (& git rev-parse HEAD).Trim()

    & $script -Mode Append -Range "$first^..$second" -RepoRoot $repo
    if ($LASTEXITCODE -ne 0) { throw "Append failed." }

    $text = Get-Content -LiteralPath "CHANGELOG.md" -Raw
    if ($text -notmatch "### Added" -or $text -notmatch "\*\*cli:\*\* Add setup menu") {
        throw "Append did not add the expected feature entry."
    }
    if ($text -match "update readme") {
        throw "Append included a docs-only commit."
    }

    & $script -Mode Promote -Version "v2026.6.4.0" -Repo "owner/repo" -RepoRoot $repo
    if ($LASTEXITCODE -ne 0) { throw "Promote failed." }

    $text = Get-Content -LiteralPath "CHANGELOG.md" -Raw
    if ($text -notmatch "## \[v2026\.6\.4\.0\]") {
        throw "Promote did not create a versioned section."
    }

    $notes = & $script -Mode Notes -ForVersion -Version "v2026.6.4.0" -RepoRoot $repo
    if ($notes -notmatch "Add setup menu") {
        throw "Notes did not return the promoted section."
    }

    Write-Host "Update-Changelog tests passed."
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $repo) {
        Remove-Item -LiteralPath $repo -Recurse -Force
    }
}
