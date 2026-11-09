#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string] $Tag = $(if ($env:TAG_NAME) { $env:TAG_NAME } else { $env:GITHUB_REF_NAME }),
    [string] $Repo = $env:GITHUB_REPOSITORY,
    [string] $Extras = $null,
    [string] $TemplateDir = $null,
    [string] $Manifest = $null,
    [string] $ZipPath = $null,
    [string] $ZipName = $null,
    [long] $ZipSize = 0,
    [string] $ZipSha256 = $null,
    [switch] $AllowEmpty,
    [string[]] $PrereleaseTags = @(),
    [switch] $SkipScrub
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Read-Utf8File {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory = $true)][string] $Path)

    $fullPath = (Resolve-Path -LiteralPath $Path).Path
    return [IO.File]::ReadAllText($fullPath, [System.Text.Encoding]::UTF8)
}

function Read-ChangelogNote {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Version)

    $path = Join-Path -Path (Get-Location) -ChildPath 'CHANGELOG.md'
    if (-not (Test-Path -LiteralPath $path)) { return $null }

    $content = Read-Utf8File -Path $path
    if ($Version) {
        $escaped = [regex]::Escape($Version)
        $versionPattern = "(?ms)^##\s+\[$escaped\][^\n]*\n(.*?)(?=^---\s*$|^##\s+|\z)"
        $versionMatch = [regex]::Match($content, $versionPattern)
        if ($versionMatch.Success) {
            $notes = $versionMatch.Groups[1].Value.Trim()
            if ($notes) { return $notes }
        }
    }

    $unreleasedPattern = '(?ms)^##\s+Unreleased\s*\n(.*?)(?=^---\s*$|^##\s+|\z)'
    $unreleasedMatch = [regex]::Match($content, $unreleasedPattern)
    if ($unreleasedMatch.Success) {
        $notes = $unreleasedMatch.Groups[1].Value.Trim()
        if ($notes -and $notes -notmatch '^_No notable changes since the last release\._$') {
            return $notes
        }
    }

    return $null
}

function Format-AuthorCredit {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Author)

    if (-not $Author) { return '' }
    if ($Author -match '\[bot]$') { return $Author }
    if ($Author -notmatch '^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$') { return $Author }
    return "[$Author](https://github.com/$Author)"
}

function Get-AuthorLoginMap {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [string] $Repo,
        [string] $PrevTag
    )

    $map = @{}
    $headSha = @(Invoke-NativeQuiet -Command 'git' -Arguments @('rev-parse', 'HEAD'))
    if ($LASTEXITCODE -ne 0 -or $headSha.Count -eq 0 -or -not $headSha[0]) { return $map }

    $comparePath = "repos/$Repo/compare/$PrevTag...$($headSha[0].Trim())"
    $compareJson = Invoke-NativeQuiet -Command 'gh' -Arguments @('api', $comparePath)
    if ($LASTEXITCODE -ne 0 -or -not $compareJson) { return $map }

    try {
        foreach ($commit in (($compareJson -join "`n") | ConvertFrom-Json).commits) {
            $authorProperty = $commit.PSObject.Properties['author']
            if ($null -eq $authorProperty -or $null -eq $authorProperty.Value) { continue }
            $loginProperty = $authorProperty.Value.PSObject.Properties['login']
            if ($null -ne $loginProperty -and $loginProperty.Value) {
                $map[$commit.sha] = $loginProperty.Value
            }
        }
    } catch [System.ArgumentException], [System.Management.Automation.RuntimeException] {
        return @{}
    }
    return $map
}

function Get-CommitEntry {
    [CmdletBinding()]
    [OutputType([pscustomobject[]])]
    param(
        [string[]] $Lines,
        [hashtable] $AuthorLoginBySha
    )

    $authorHandleMap = @{
        'WhyKnot' = 'RealWhyKnot'
        'Sticks'  = 'SticksDev'
    }

    foreach ($line in $Lines) {
        if ($line -match '\[skip changelog\]') { continue }
        $parts = $line -split "`t", 4
        if ($parts.Count -lt 4) { continue }
        $sha = $parts[0]
        $author = $parts[2]
        if ($AuthorLoginBySha.ContainsKey($sha)) {
            $author = $AuthorLoginBySha[$sha]
        } elseif ($authorHandleMap.ContainsKey($author)) {
            $author = $authorHandleMap[$author]
        }

        $subject = $parts[3] -replace '\s*\(\d{4}\.\d+\.\d+\.\d+-[A-Fa-f0-9]+\)\s*', ' '
        $subject = $subject.Trim() -replace '\s{2,}', ' '

        [pscustomobject]@{
            Sha     = $sha
            Short   = $parts[1]
            Author  = $author
            Subject = $subject
        }
    }
}

function Get-Category {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([string] $Subject)

    if ($Subject -match '^feat(\(.+?\))?!?:') { return @{ Order = 1; Name = 'Features' } }
    if ($Subject -match '^fix(\(.+?\))?!?:') { return @{ Order = 2; Name = 'Bug Fixes' } }
    if ($Subject -match '^perf(\(.+?\))?!?:') { return @{ Order = 3; Name = 'Performance' } }
    if ($Subject -match '^refactor(\(.+?\))?!?:') { return @{ Order = 4; Name = 'Refactors' } }
    if ($Subject -match '^revert(\(.+?\))?!?:') { return @{ Order = 5; Name = 'Reverts' } }
    if ($Subject -match '^docs(\(.+?\))?!?:') { return @{ Order = 6; Name = 'Documentation' } }
    if ($Subject -match '^style(\(.+?\))?!?:') { return @{ Order = 7; Name = 'Style' } }
    if ($Subject -match '^test(\(.+?\))?!?:') { return @{ Order = 8; Name = 'Tests' } }
    if ($Subject -match '^ci(\(.+?\))?!?:') { return @{ Order = 9; Name = 'CI' } }
    if ($Subject -match '^build(\(.+?\))?!?:') { return @{ Order = 10; Name = 'Build' } }
    if ($Subject -match '^chore(\(.+?\))?!?:') { return @{ Order = 11; Name = 'Chores' } }
    return @{ Order = 99; Name = 'Other Changes' }
}

function Test-ConventionalSubject {
    [CmdletBinding()]
    [OutputType([bool])]
    param([string] $Subject)

    return $Subject -match '^(feat|fix|perf|refactor|revert|docs|style|test|ci|build|chore)(\(.+?\))?!?:'
}

function Get-TagCommitSha {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Tag)

    $tagSha = Invoke-NativeQuiet -Command 'git' -Arguments @('rev-parse', "$Tag^{}")
    if ($LASTEXITCODE -eq 0 -and $tagSha) {
        return $tagSha.Trim()
    }
    return ''
}

function Expand-Token {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $Text,
        [hashtable] $Map
    )

    if (-not $Text) { return $Text }
    foreach ($key in $Map.Keys) {
        $val = $Map[$key]
        if ($null -eq $val) { $val = '' }
        $Text = $Text.Replace($key, $val)
    }
    return $Text
}

function Read-TemplateSection {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $Name,
        [string] $Dir,
        [hashtable] $TokenMap
    )

    $path = Join-Path -Path $Dir -ChildPath "$Name.md"
    if (-not (Test-Path -LiteralPath $path)) {
        Write-Host "::warning::Release-body template missing: $path. Section '$Name' will not render."
        return $null
    }
    $content = (Read-Utf8File -Path $path).Trim()
    if (-not $content) { return $null }
    return (Expand-Token -Text $content -Map $TokenMap)
}

function Format-ChangeList {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [object[]] $Entries,
        [bool] $UseGroups
    )

    if (-not $UseGroups) {
        foreach ($e in $Entries) {
            "- $($e.Subject) by $(Format-AuthorCredit -Author $e.Author) in $($e.Short)"
        }
        ''
        return
    }

    $tagged = foreach ($e in $Entries) {
        $cat = Get-Category -Subject $e.Subject
        [pscustomobject]@{ Order = $cat['Order']; Name = $cat['Name']; Entry = $e }
    }
    $groups = $tagged | Group-Object Name | Sort-Object { ($_.Group | Select-Object -First 1).Order }
    foreach ($g in $groups) {
        "### $($g.Name)"
        foreach ($t in $g.Group) {
            $e = $t.Entry
            "- $($e.Subject) by $(Format-AuthorCredit -Author $e.Author) in $($e.Short)"
        }
        ''
    }
}

function ConvertTo-AsciiText {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Text)

    $asciiSubs = @(
        @{ Pattern = [string][char]0x2014; Replacement = '--' }
        @{ Pattern = [string][char]0x2013; Replacement = '-' }
        @{ Pattern = [string][char]0x2026; Replacement = '...' }
        @{ Pattern = [string][char]0x201C; Replacement = '"' }
        @{ Pattern = [string][char]0x201D; Replacement = '"' }
        @{ Pattern = [string][char]0x2018; Replacement = "'" }
        @{ Pattern = [string][char]0x2019; Replacement = "'" }
        @{ Pattern = [string][char]0x00A0; Replacement = ' ' }
        @{ Pattern = [string][char]0x2022; Replacement = '*' }
        @{ Pattern = [string][char]0x00D7; Replacement = 'x' }
        @{ Pattern = [string][char]0x2192; Replacement = '->' }
        @{ Pattern = [string][char]0x2190; Replacement = '<-' }
        @{ Pattern = [string][char]0x21D2; Replacement = '=>' }
        @{ Pattern = [string][char]0x21D0; Replacement = '<=' }
        @{ Pattern = [string][char]0x00A7; Replacement = 'section' }
        @{ Pattern = [string][char]0x00B6; Replacement = 'paragraph' }
    )
    foreach ($sub in $asciiSubs) {
        $Text = $Text.Replace($sub['Pattern'], $sub['Replacement'])
    }
    return $Text
}

function Assert-AsciiBody {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Body)

    $report = [System.Collections.Generic.List[string]]::new()
    $lineNumber = 0
    foreach ($line in ($Body -split "`r?`n")) {
        $lineNumber++
        for ($i = 0; $i -lt $line.Length; $i++) {
            $code = [int][char]$line[$i]
            $isAllowed = ($code -ge 0x20 -and $code -le 0x7E) -or $code -eq 9
            if (-not $isAllowed) {
                $report.Add("  line $lineNumber col $($i + 1): $('U+{0:X4}' -f $code) in: $line")
            }
        }
    }
    if ($report.Count -eq 0) { return }

    $fix = @(
        'Fix: amend the offending commit subject (or extras file) to use ASCII equivalents.',
        'Common substitutes are pre-mapped in Generate-ReleaseNotes.ps1; if a new character',
        "trips this, add it to `$asciiSubs and try again."
    ) -join ' '
    throw "Non-ASCII characters in release body after normalisation:`n$($report -join "`n")`n$fix"
}

Import-Module -Name (Join-Path $PSScriptRoot 'ReleaseSlice.psm1') -Force

if (-not $Tag) { throw 'No tag provided (pass -Tag or set TAG_NAME / GITHUB_REF_NAME).' }

if (-not $Extras) {
    $Extras = Join-Path -Path (Get-Location) -ChildPath ".github/release-extras/$Tag.md"
}
if (-not $TemplateDir) {
    $TemplateDir = Join-Path -Path (Get-Location) -ChildPath '.github/release-template'
}

$prevInfo = Resolve-PrevTagForSlice -Tag $Tag -Repo $Repo -AdditionalPrereleaseTags $PrereleaseTags
$prevTag = $prevInfo['Tag']
$range = $prevInfo['Display']

$changelogNotes = $null
if ($prevInfo['Source'] -eq 'first-release') {
    $changelogNotes = Read-ChangelogNote -Version $Tag
    if (-not $changelogNotes) {
        if (-not $AllowEmpty) {
            $message = @(
                "No prior tag or release exists, and CHANGELOG.md has no notes for $Tag or Unreleased.",
                'For the first release, write curated public notes under ## Unreleased before tagging.'
            ) -join ' '
            throw $message
        }
        $changelogNotes = '_First release; see commit log for details._'
    }
}

$raw = @()
if (-not $changelogNotes) {
    $logArguments = [System.Collections.Generic.List[string]]::new()
    $logArguments.Add('log')
    $logArguments.AddRange([string[]]@($prevInfo['LogArgs']))
    $logArguments.AddRange([string[]]@('--no-merges', "--pretty=format:%H`t%h`t%an`t%s"))
    $raw = Invoke-NativeQuiet -Command 'git' -Arguments $logArguments.ToArray()
    if ($LASTEXITCODE -ne 0) { $raw = @() }
}

$lines = @()
if ($raw) { $lines = @($raw -split "`r?`n" | Where-Object { $_ }) }

$authorLoginBySha = @{}
if ($Repo -and $prevTag -and -not $changelogNotes) {
    $authorLoginBySha = Get-AuthorLoginMap -Repo $Repo -PrevTag $prevTag
    if ($authorLoginBySha.Count -eq 0) {
        Write-Host '::warning::Could not resolve GitHub logins from the compare API; using the author handle map.'
    }
}

$entries = @(Get-CommitEntry -Lines $lines -AuthorLoginBySha $authorLoginBySha)

if (-not $changelogNotes -and $entries.Count -eq 0) {
    if ($AllowEmpty) {
        "## What's Changed`n`n_First release; see commit log for details._`n"
        exit 0
    }
    $message = @(
        "No commits found in range $range.",
        'Either the previous tag is misdetected, every commit in the range',
        'carries [skip changelog], or the tag points at an empty branch.',
        'Pass -AllowEmpty for a first release. Otherwise amend the offending',
        'commits or push a real change before tagging.'
    ) -join ' '
    throw $message
}

$useGroups = $false
if (-not $changelogNotes) {
    $nonConforming = @($entries | Where-Object { -not (Test-ConventionalSubject -Subject $_.Subject) })
    if ($nonConforming.Count -gt 0) {
        $warning = @(
            "::warning::$($nonConforming.Count) commit(s) in range $range do not follow",
            "conventional-commit prefixes; bucketed under 'Other Changes':"
        ) -join ' '
        Write-Host $warning
        foreach ($e in $nonConforming) {
            Write-Host "::warning::  $($e.Short)  $($e.Subject)"
        }
    }
    $useGroups = $nonConforming.Count -lt $entries.Count
}

$ownerOnly = ''
$repoShort = ''
if ($Repo -and ($Repo -match '/')) {
    $parts = $Repo -split '/', 2
    $ownerOnly = $parts[0]
    $repoShort = $parts[1]
} elseif ($Repo) {
    $repoShort = $Repo
}

$tagCommitSha = Get-TagCommitSha -Tag $Tag
$tagCommitShort = ''
if ($tagCommitSha.Length -ge 12) { $tagCommitShort = $tagCommitSha.Substring(0, 12) }
$zipNameToken = if ($ZipName) { $ZipName } elseif ($ZipPath) { (Split-Path -Leaf $ZipPath) } else { '' }
$tokens = @{
    '{tag}'              = $Tag
    '{version}'          = ($Tag -replace '^v', '')
    '{owner}'            = $ownerOnly
    '{repo}'             = $repoShort
    '{full-repo}'        = $Repo
    '{commit-sha}'       = $tagCommitSha
    '{commit-sha-short}' = $tagCommitShort
    '{prior-tag}'        = $prevTag
    '{zip-name}'         = $zipNameToken
}

$sb = [System.Text.StringBuilder]::new()
if ($repoShort) {
    $null = $sb.AppendLine("# $repoShort $Tag")
    $null = $sb.AppendLine()
}
$null = $sb.AppendLine("## What's Changed")
$null = $sb.AppendLine()

if ($changelogNotes) {
    $null = $sb.AppendLine($changelogNotes)
    $null = $sb.AppendLine()
} else {
    foreach ($changeLine in @(Format-ChangeList -Entries $entries -UseGroups $useGroups)) {
        $null = $sb.AppendLine($changeLine)
    }
}

if ($Repo -and $prevTag) {
    $null = $sb.AppendLine("**Full Changelog**: https://github.com/$Repo/compare/$prevTag...$Tag")
}

$hasManifest = $Manifest -and (Test-Path -LiteralPath $Manifest)
if ($ZipPath -and $ZipSha256 -and $ZipSize -gt 0 -and $hasManifest) {
    $zipNameForLine = if ($zipNameToken) { $zipNameToken } else { Split-Path -Leaf $ZipPath }
    $integrityName = $zipNameForLine -replace '\.zip$', '.integrity.tsv'
    $manifestName = Split-Path -Leaf $Manifest
    $integrityLine = @(
        "Release SHA256 hashes are attached as ``$integrityName``.",
        "Package contents are listed in ``$manifestName``."
    ) -join ' '
    $null = $sb.AppendLine()
    $null = $sb.AppendLine('## File integrity')
    $null = $sb.AppendLine()
    $null = $sb.AppendLine($integrityLine)
} elseif ($Manifest -or $ZipPath -or $ZipSha256) {
    $warning = @(
        '::warning::File-integrity section skipped:',
        '-Manifest, -ZipPath, -ZipSize, and -ZipSha256 must all be set.',
        "Got Manifest='$Manifest' ZipPath='$ZipPath' ZipSize=$ZipSize ZipSha256='$ZipSha256'."
    ) -join ' '
    Write-Host $warning
}

foreach ($name in @('links', 'install', 'uninstall', 'what-you-need-to-do')) {
    $section = Read-TemplateSection -Name $name -Dir $TemplateDir -TokenMap $tokens
    if ($section) {
        $null = $sb.AppendLine()
        $null = $sb.AppendLine($section)
    }
}

if (Test-Path -LiteralPath $Extras) {
    $extrasContent = (Read-Utf8File -Path $Extras).Trim()
    if ($extrasContent) {
        $null = $sb.AppendLine()
        $null = $sb.AppendLine('---')
        $null = $sb.AppendLine()
        $null = $sb.AppendLine('## Additional notes')
        $null = $sb.AppendLine()
        $null = $sb.AppendLine($extrasContent)
    }
}

$body = ConvertTo-AsciiText -Text $sb.ToString().TrimEnd()
if (-not $SkipScrub) {
    Assert-AsciiBody -Body $body
}

$body
exit 0
