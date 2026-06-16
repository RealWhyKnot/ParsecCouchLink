#!/usr/bin/env pwsh
<#
.SYNOPSIS
  Maintains CHANGELOG.md from conventional commit subjects.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Append", "Promote", "Notes")]
    [string]$Mode,

    [string]$Range,
    [string]$Version,
    [switch]$ForVersion,
    [string]$Repo = $env:GITHUB_REPOSITORY,
    [string]$RepoRoot,
    [datetime]$NowUtc = ([datetime]::UtcNow)
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $RepoRoot) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

$ChangelogPath = Join-Path $RepoRoot "CHANGELOG.md"
if (-not (Test-Path -LiteralPath $ChangelogPath)) {
    throw "CHANGELOG.md not found at $ChangelogPath"
}

function New-Utf8NoBomEncoding {
    return New-Object System.Text.UTF8Encoding -ArgumentList $false
}

function Read-TextFile {
    param([Parameter(Mandatory = $true)][string]$Path)
    return [System.IO.File]::ReadAllText($Path, (New-Utf8NoBomEncoding))
}

function Write-TextFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )
    [System.IO.File]::WriteAllText($Path, $Content, (New-Utf8NoBomEncoding))
}

function Get-CentralTimeZone {
    foreach ($id in @("Central Standard Time", "America/Chicago")) {
        try {
            return [System.TimeZoneInfo]::FindSystemTimeZoneById($id)
        } catch {
            continue
        }
    }

    throw "Could not resolve the America/Chicago release time zone."
}

function Get-ReleaseDateStamp {
    param(
        [datetime]$NowUtc,
        [string]$Format
    )

    $utc = $NowUtc
    if ($utc.Kind -ne [System.DateTimeKind]::Utc) {
        $utc = $utc.ToUniversalTime()
    }

    $central = [System.TimeZoneInfo]::ConvertTimeFromUtc($utc, (Get-CentralTimeZone))
    return $central.ToString($Format, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Strip-BuildStamp {
    param([Parameter(Mandatory = $true)][string]$Subject)
    return ($Subject -replace " \(\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?\)$", "").Trim()
}

function ConvertTo-ChangelogEntry {
    param(
        [Parameter(Mandatory = $true)][string]$Sha,
        [Parameter(Mandatory = $true)][string]$Subject
    )

    $clean = Strip-BuildStamp -Subject $Subject
    if ([string]::IsNullOrWhiteSpace($clean)) { return $null }
    if ($clean -match "\[skip changelog\]") { return $null }
    if ($clean -match "^Merge ") { return $null }

    $pattern = "^(?<type>feat|fix|perf|refactor|style|revert|docs|build|ci|chore|test)(?:\((?<scope>[^)]+)\))?(?<bang>!)?:\s+(?<desc>.+)$"
    $m = [regex]::Match($clean, $pattern)
    if (-not $m.Success) {
        return @{
            Bucket = "Changed"
            Bullet = "- $clean (" + $Sha.Substring(0, 7) + ")"
        }
    }

    $type = $m.Groups["type"].Value
    $scope = $m.Groups["scope"].Value
    $desc = $m.Groups["desc"].Value
    $isBreaking = $m.Groups["bang"].Success

    if ($desc.Length -gt 0) {
        $desc = $desc.Substring(0, 1).ToUpperInvariant() + $desc.Substring(1)
    }

    $bucket = switch ($type) {
        "feat" { "Added" }
        "fix" { "Fixed" }
        "perf" { "Changed" }
        "refactor" { "Changed" }
        "style" { "Changed" }
        "revert" { "Changed" }
        "chore" {
            if ($scope -and $scope -match "^deps") { "Changed" } else { $null }
        }
        default { $null }
    }

    if (-not $bucket) { return $null }
    if ($isBreaking) { $bucket = "Breaking" }

    $prefix = if ($scope) { "**${scope}:** " } else { "" }
    return @{
        Bucket = $bucket
        Bullet = "- $prefix$desc (" + $Sha.Substring(0, 7) + ")"
    }
}

$BucketOrder = @("Breaking", "Added", "Changed", "Fixed")

function Find-UnreleasedSection {
    param([Parameter(Mandatory = $true)][string]$Content)

    $lines = $Content -split "\r?\n"
    $startIdx = -1
    for ($i = 0; $i -lt $lines.Length; $i++) {
        if ($lines[$i] -match "^##\s+Unreleased\s*$") {
            $startIdx = $i
            break
        }
    }
    if ($startIdx -lt 0) { return $null }

    $endIdx = $lines.Length
    for ($j = $startIdx + 1; $j -lt $lines.Length; $j++) {
        if ($lines[$j] -match "^---\s*$" -or $lines[$j] -match "^##\s+") {
            $endIdx = $j
            break
        }
    }

    return @{
        Lines = $lines
        StartIdx = $startIdx
        EndIdx = $endIdx
    }
}

function Read-Buckets {
    param([string[]]$BodyLines)

    $buckets = [ordered]@{}
    $current = $null
    foreach ($line in $BodyLines) {
        if ($line -match "^###\s+(?<name>.+?)\s*$") {
            $current = $matches["name"]
            if (-not $buckets.Contains($current)) { $buckets[$current] = @() }
            continue
        }
        if ($line -match "^\s*- " -and $current) {
            $buckets[$current] += $line
        }
    }
    return $buckets
}

function Write-Buckets {
    param([hashtable]$Buckets)

    $hasEntries = $false
    foreach ($key in $Buckets.Keys) {
        if ($Buckets[$key].Count -gt 0) {
            $hasEntries = $true
            break
        }
    }
    if (-not $hasEntries) {
        return @("", "_No notable changes since the last release._", "")
    }

    $out = @("")
    $emitted = @{}
    foreach ($name in $BucketOrder) {
        if ($Buckets.Contains($name) -and $Buckets[$name].Count -gt 0) {
            $out += "### $name"
            $out += $Buckets[$name]
            $out += ""
            $emitted[$name] = $true
        }
    }
    foreach ($name in $Buckets.Keys) {
        if (-not $emitted.Contains($name) -and $Buckets[$name].Count -gt 0) {
            $out += "### $name"
            $out += $Buckets[$name]
            $out += ""
        }
    }
    return $out
}

function Update-Unreleased {
    param([hashtable]$NewEntries)

    $content = Read-TextFile -Path $ChangelogPath
    $section = Find-UnreleasedSection -Content $content
    if (-not $section) {
        throw "CHANGELOG.md is missing the '## Unreleased' section."
    }

    $lines = $section["Lines"]
    $bodyStart = $section["StartIdx"] + 1
    $bodyEnd = $section["EndIdx"] - 1
    $bodyLines = if ($bodyEnd -ge $bodyStart) { $lines[$bodyStart..$bodyEnd] } else { @() }
    $buckets = Read-Buckets -BodyLines $bodyLines

    foreach ($bucket in $NewEntries.Keys) {
        if (-not $buckets.Contains($bucket)) { $buckets[$bucket] = @() }
        foreach ($bullet in $NewEntries[$bucket]) {
            $shortSha = if ($bullet -match "\(([a-f0-9]{7})\)\s*$") { $matches[1] } else { $null }
            $seen = $false
            if ($shortSha) {
                foreach ($existing in $buckets[$bucket]) {
                    if ($existing -match "\($shortSha\)") {
                        $seen = $true
                        break
                    }
                }
            }
            if (-not $seen) {
                $buckets[$bucket] += $bullet
            }
        }
    }

    $before = if ($section["StartIdx"] -gt 0) { $lines[0..($section["StartIdx"])] } else { @($lines[0]) }
    $after = if ($section["EndIdx"] -lt $lines.Length) { $lines[$section["EndIdx"]..($lines.Length - 1)] } else { @() }
    $newLines = @()
    $newLines += $before
    $newLines += Write-Buckets -Buckets $buckets
    $newLines += $after

    Write-TextFile -Path $ChangelogPath -Content ($newLines -join "`n")
}

if ($Mode -eq "Append") {
    if (-not $Range) { throw "Append mode requires -Range." }

    Push-Location $RepoRoot
    try {
        $log = & git log --no-merges --format="%H%x09%s%x09%ae" $Range 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "git log failed for range '$Range'; leaving CHANGELOG.md unchanged."
            return
        }
    } finally {
        Pop-Location
    }

    if (-not $log) {
        Write-Host "No commits in range $Range."
        return
    }

    $newEntries = @{}
    $considered = 0
    $included = 0
    foreach ($line in ($log -split "\r?\n")) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $parts = $line -split "`t", 3
        if ($parts.Length -lt 3) { continue }
        $sha = $parts[0]
        $subject = $parts[1]
        $email = $parts[2]
        $considered++
        if ($email -match "github-actions\[bot\]" -or $email -match "noreply@github.com") { continue }

        $entry = ConvertTo-ChangelogEntry -Sha $sha -Subject $subject
        if (-not $entry) { continue }

        $bucket = $entry["Bucket"]
        if (-not $newEntries.ContainsKey($bucket)) { $newEntries[$bucket] = @() }
        $newEntries[$bucket] += $entry["Bullet"]
        $included++
    }

    Write-Host "Considered $considered commit(s), included $included."
    if ($included -eq 0) { return }

    Update-Unreleased -NewEntries $newEntries
    Write-Host "Updated CHANGELOG.md."
    return
}

if ($Mode -eq "Promote") {
    if (-not $Version) { throw "Promote mode requires -Version." }
    if (-not $Repo) { throw "Promote mode requires -Repo or GITHUB_REPOSITORY." }

    $today = Get-ReleaseDateStamp -NowUtc $NowUtc -Format "yyyy-MM-dd"
    $heading = "## [$Version](https://github.com/$Repo/releases/tag/$Version) -- $today"
    $content = Read-TextFile -Path $ChangelogPath
    $section = Find-UnreleasedSection -Content $content
    if (-not $section) {
        throw "CHANGELOG.md is missing the '## Unreleased' section."
    }

    $lines = $section["Lines"]
    $bodyStart = $section["StartIdx"] + 1
    $bodyEnd = $section["EndIdx"] - 1
    $bodyLines = if ($bodyEnd -ge $bodyStart) { $lines[$bodyStart..$bodyEnd] } else { @() }

    $hasRealEntry = $false
    foreach ($line in $bodyLines) {
        if ($line -match "^\s*- " -or $line -match "^###\s+") {
            $hasRealEntry = $true
            break
        }
    }
    if (-not $hasRealEntry) {
        $bodyLines = @("", "_Maintenance release; see commit log for details._", "")
    }

    $before = if ($section["StartIdx"] -gt 0) { $lines[0..($section["StartIdx"] - 1)] } else { @() }
    $after = if ($section["EndIdx"] -lt $lines.Length) { $lines[$section["EndIdx"]..($lines.Length - 1)] } else { @() }
    $newLines = @()
    $newLines += $before
    $newLines += "## Unreleased"
    $newLines += ""
    $newLines += "_No notable changes since the last release._"
    $newLines += ""
    $newLines += "---"
    $newLines += ""
    $newLines += $heading
    $newLines += $bodyLines
    $newLines += $after

    Write-TextFile -Path $ChangelogPath -Content ($newLines -join "`n")
    Write-Host "Promoted Unreleased to $Version."
    return
}

if ($Mode -eq "Notes") {
    $content = Read-TextFile -Path $ChangelogPath
    if ($ForVersion) {
        if (-not $Version) { throw "Notes -ForVersion requires -Version." }
        $escaped = [regex]::Escape($Version)
        $pattern = "(?ms)^##\s+\[" + $escaped + "\][^\n]*\n(.*?)(?=^---\s*$|^##\s+|\z)"
        $match = [regex]::Match($content, $pattern)
        if (-not $match.Success) {
            throw "No changelog section found for $Version."
        }
        Write-Output $match.Groups[1].Value.Trim()
        return
    }

    $section = Find-UnreleasedSection -Content $content
    if (-not $section) { throw "No '## Unreleased' section found." }
    $lines = $section["Lines"]
    $bodyStart = $section["StartIdx"] + 1
    $bodyEnd = $section["EndIdx"] - 1
    $bodyLines = if ($bodyEnd -ge $bodyStart) { $lines[$bodyStart..$bodyEnd] } else { @() }
    Write-Output (($bodyLines -join "`n").Trim())
    return
}
