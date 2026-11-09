#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Git {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory = $true)][string[]] $Arguments)

    & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Write-TestFile {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Path,
        [AllowEmptyString()][string] $Content
    )

    $fullPath = Join-Path (Get-Location) $Path
    $dir = Split-Path -Parent $fullPath
    if ($dir -and -not (Test-Path -LiteralPath $dir)) {
        $null = New-Item -ItemType Directory -Force -Path $dir
    }
    [System.IO.File]::WriteAllText($fullPath, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function Add-TestCommit {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Path,
        [string] $Content,
        [string] $Subject
    )

    Write-TestFile -Path $Path -Content $Content
    Invoke-Git -Arguments @('add', '--', $Path)
    Invoke-Git -Arguments @('commit', '-q', '-m', $Subject)
}

function Assert-Contain {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Text,
        [string] $Expected
    )

    if (-not $Text.Contains($Expected)) {
        throw "Expected release notes to contain '$Expected'."
    }
}

function Assert-NotContain {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Text,
        [string] $Unexpected
    )

    if ($Text.Contains($Unexpected)) {
        throw "Expected release notes not to contain '$Unexpected'."
    }
}

$generator = Join-Path $PSScriptRoot 'Generate-ReleaseNotes.ps1'
$tempName = 'ParsecCouchLink.ReleaseNotes.' + [Guid]::NewGuid().ToString('N')
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) $tempName
$pushedLocation = $false

try {
    $null = New-Item -ItemType Directory -Force -Path $tempRoot
    Push-Location -LiteralPath $tempRoot
    $pushedLocation = $true

    Invoke-Git -Arguments @('init', '-q')
    Invoke-Git -Arguments @('config', 'core.autocrlf', 'false')
    Invoke-Git -Arguments @('config', 'user.name', 'WhyKnot')
    Invoke-Git -Arguments @('config', 'user.email', 'whyknot@example.invalid')

    $null = New-Item -ItemType Directory -Force -Path '.github/release-template'
    Write-TestFile -Path '.github/release-template/links.md' -Content @'
## More

- **README:** <https://github.com/{full-repo}>
- **Changelog:** <https://github.com/{full-repo}/blob/main/CHANGELOG.md>
- **Source:** commit `{commit-sha-short}` on `main`
- **License:** [GPL-3.0](https://github.com/{full-repo}/blob/main/LICENSE)
'@
    Write-TestFile -Path '.github/release-template/install.md' -Content @'
## Install

Download `{zip-name}` and run setup.
'@
    Write-TestFile -Path '.github/release-template/uninstall.md' -Content ''
    Write-TestFile -Path '.github/release-template/what-you-need-to-do.md' -Content ''

    $seedChangelog = "# Changelog`n`n## Unreleased`n`n_No notable changes since the last release._`n"
    Add-TestCommit -Path 'CHANGELOG.md' -Content $seedChangelog -Subject 'chore: initial changelog'
    Invoke-Git -Arguments @('tag', '-a', 'v2026.5.1.0', '-m', 'ParsecCouchLink 2026.5.1.0')

    $subject = 'fix(helper): plain prerelease fallback'
    Add-TestCommit -Path 'src.txt' -Content "plain-prerelease`n" -Subject $subject
    Invoke-Git -Arguments @('tag', '-a', 'v2026.5.2.0', '-m', 'ParsecCouchLink 2026.5.2.0')

    Add-TestCommit -Path 'src.txt' -Content "beta`n" -Subject 'fix(relay): beta port fallback'
    Invoke-Git -Arguments @('tag', '-a', 'v2026.5.2.1-beta', '-m', 'ParsecCouchLink 2026.5.2.1-beta')

    Add-TestCommit -Path 'src.txt' -Content "stable`n" -Subject 'fix(vrclog): stable startup tail'
    Invoke-Git -Arguments @('tag', '-a', 'v2026.5.3.0', '-m', 'ParsecCouchLink 2026.5.3.0')

    $common = @{
        Repo           = 'RealWhyKnot/ParsecCouchLink'
        TemplateDir    = Join-Path $tempRoot '.github/release-template'
        PrereleaseTags = 'v2026.5.2.0'
        SkipScrub      = $true
    }

    $stableNotes = (& $generator -Tag 'v2026.5.3.0' @common) -join "`n"

    Assert-Contain -Text $stableNotes -Expected 'fix(helper): plain prerelease fallback'
    Assert-Contain -Text $stableNotes -Expected 'fix(relay): beta port fallback'
    Assert-Contain -Text $stableNotes -Expected 'fix(vrclog): stable startup tail'
    Assert-Contain -Text $stableNotes -Expected 'compare/v2026.5.1.0...v2026.5.3.0'
    Assert-NotContain -Text $stableNotes -Unexpected 'compare/v2026.5.2.0...v2026.5.3.0'
    Assert-NotContain -Text $stableNotes -Unexpected 'compare/v2026.5.2.1-beta...v2026.5.3.0'

    $betaNotes = (& $generator -Tag 'v2026.5.2.1-beta' @common) -join "`n"

    Assert-Contain -Text $betaNotes -Expected 'fix(relay): beta port fallback'
    Assert-Contain -Text $betaNotes -Expected 'compare/v2026.5.2.0...v2026.5.2.1-beta'
    Assert-NotContain -Text $betaNotes -Unexpected 'fix(helper): plain prerelease fallback'
    Assert-NotContain -Text $betaNotes -Unexpected 'fix(vrclog): stable startup tail'

    $plainPrereleaseNotes = (& $generator -Tag 'v2026.5.2.0' @common) -join "`n"

    Assert-Contain -Text $plainPrereleaseNotes -Expected 'fix(helper): plain prerelease fallback'
    Assert-Contain -Text $plainPrereleaseNotes -Expected 'compare/v2026.5.1.0...v2026.5.2.0'
    Assert-NotContain -Text $plainPrereleaseNotes -Unexpected 'fix(relay): beta port fallback'

    $null = New-Item -ItemType Directory -Force -Path 'artifacts'
    $zipPath = Join-Path $tempRoot 'artifacts/ParsecCouchLink-v2026.5.3.0.zip'
    $manifestPath = Join-Path $tempRoot 'artifacts/ParsecCouchLink-v2026.5.3.0.manifest.tsv'
    $zipSha = 'E8966F33BE8246922756E3E8234CF8309FB6D3151665594203F53BBF5725164B'
    $exeSha = 'A24EA7D3DF2B0718AFF60B5B9EBEBDF590ED4938D81A6B08CDEC7A880B326B0C'
    $manifestText = "path`tbytes`tsha256`ncouchlink.exe`t123`t$exeSha`n"
    Write-TestFile -Path 'artifacts/ParsecCouchLink-v2026.5.3.0.zip' -Content 'zip'
    Write-TestFile -Path 'artifacts/ParsecCouchLink-v2026.5.3.0.manifest.tsv' -Content $manifestText

    $integrity = @{
        ZipPath   = $zipPath
        ZipName   = 'ParsecCouchLink-v2026.5.3.0.zip'
        ZipSize   = 157017922
        ZipSha256 = $zipSha
        Manifest  = $manifestPath
    }
    $integrityNotes = (& $generator -Tag 'v2026.5.3.0' @common @integrity) -join "`n"

    $changelogLink = '- **Changelog:** <https://github.com/RealWhyKnot/ParsecCouchLink/blob/main/CHANGELOG.md>'
    Assert-NotContain -Text $integrityNotes -Unexpected "SHA256: $zipSha"
    Assert-Contain -Text $integrityNotes -Expected 'ParsecCouchLink-v2026.5.3.0.integrity.tsv'
    Assert-Contain -Text $integrityNotes -Expected 'ParsecCouchLink-v2026.5.3.0.manifest.tsv'
    Assert-Contain -Text $integrityNotes -Expected $changelogLink
    Assert-NotContain -Text $integrityNotes -Unexpected 'couchlink.exe                        '

    Write-Host 'Generate-ReleaseNotes tests passed.'
} finally {
    if ($pushedLocation) {
        Pop-Location
    }
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

exit 0
