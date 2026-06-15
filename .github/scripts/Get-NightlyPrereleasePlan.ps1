#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$RepoRoot = "",
    [string]$Tag = "",
    [string]$ReleaseStatePath = "",
    [string]$Today = "",
    [string]$OutputJsonPath = ""
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    param([string]$Value)

    if (-not [string]::IsNullOrWhiteSpace($Value)) {
        return (Resolve-Path -LiteralPath $Value).Path
    }

    return (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..")).Path
}

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)

    $output = & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
    return @($output)
}

function Test-GitTagExists {
    param([string]$Name)

    if ([string]::IsNullOrWhiteSpace($Name)) {
        return $false
    }

    & git rev-parse --verify --quiet "refs/tags/$Name^{commit}" *> $null
    return ($LASTEXITCODE -eq 0)
}

function Normalize-RepoPath {
    param([string]$Path)

    $normalized = $Path -replace "\\", "/"
    while ($normalized.StartsWith("./", [System.StringComparison]::Ordinal)) {
        $normalized = $normalized.Substring(2)
    }
    return $normalized
}

function Test-PathMatchesPattern {
    param(
        [string]$Path,
        [string]$Pattern
    )

    $repoPath = Normalize-RepoPath -Path $Path
    $repoPattern = Normalize-RepoPath -Path $Pattern

    if ($repoPattern.EndsWith("/", [System.StringComparison]::Ordinal)) {
        return $repoPath.StartsWith($repoPattern, [System.StringComparison]::OrdinalIgnoreCase)
    }

    return [string]::Equals($repoPath, $repoPattern, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-AnyPathMatches {
    param(
        [string[]]$Files,
        [string[]]$Patterns
    )

    foreach ($file in $Files) {
        foreach ($pattern in $Patterns) {
            if (Test-PathMatchesPattern -Path $file -Pattern $pattern) {
                return $true
            }
        }
    }

    return $false
}

function Get-ReleasablePatterns {
    return @(
        ".github/scripts/",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/nightly-prerelease.yml",
        ".clang-format",
        ".editorconfig",
        "build.ps1",
        "setup.ps1",
        "bridge/",
        "pico-bridge/",
        "scripts/",
        "tools/",
        "README.md",
        "LICENSE",
        "NOTICE"
    )
}

function Read-ReleaseState {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $null
    }

    $resolved = Resolve-Path -LiteralPath $Path
    $json = Get-Content -LiteralPath $resolved.Path -Raw
    if ([string]::IsNullOrWhiteSpace($json)) {
        return $null
    }

    return ($json | ConvertFrom-Json)
}

function Get-StateReleaseTag {
    param([object]$ReleaseState)

    if ($null -eq $ReleaseState) {
        return ""
    }

    foreach ($name in @("latest_release_tag", "latest_release", "tag")) {
        $property = $ReleaseState.PSObject.Properties[$name]
        if ($null -ne $property -and $null -ne $property.Value) {
            return [string]$property.Value
        }
    }

    return ""
}

function Get-LatestReleaseTag {
    param(
        [object]$ReleaseState,
        [string]$Repo,
        [string]$ExcludeTag
    )

    $stateTag = Get-StateReleaseTag -ReleaseState $ReleaseState
    if (-not [string]::IsNullOrWhiteSpace($stateTag)) {
        if ($stateTag -ne $ExcludeTag) {
            return $stateTag
        }
        return ""
    }

    $json = & gh api "repos/$Repo/releases?per_page=50"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to read releases for $Repo"
    }

    $releases = @($json | ConvertFrom-Json)
    foreach ($release in $releases) {
        $draftProperty = $release.PSObject.Properties["draft"]
        $draft = $false
        if ($null -ne $draftProperty) {
            $draft = [bool]$draftProperty.Value
        }

        $tagProperty = $release.PSObject.Properties["tag_name"]
        $releaseTag = ""
        if ($null -ne $tagProperty -and $null -ne $tagProperty.Value) {
            $releaseTag = [string]$tagProperty.Value
        }

        if (-not $draft -and -not [string]::IsNullOrWhiteSpace($releaseTag) -and $releaseTag -ne $ExcludeTag) {
            return $releaseTag
        }
    }

    return ""
}

function Get-ChangedFilesSinceTag {
    param([string]$BaseTag)

    if (-not [string]::IsNullOrWhiteSpace($BaseTag) -and (Test-GitTagExists -Name $BaseTag)) {
        return @(Invoke-Git -Arguments @("diff", "--name-only", "$BaseTag..HEAD", "--"))
    }

    return @(Invoke-Git -Arguments @("ls-files"))
}

function Get-NextBetaTag {
    param([string]$DateStamp)

    if ([string]::IsNullOrWhiteSpace($DateStamp)) {
        $DateStamp = (Get-Date).ToUniversalTime().ToString("yyyy.M.d", [System.Globalization.CultureInfo]::InvariantCulture)
    }

    $escapedDate = [regex]::Escape($DateStamp)
    $pattern = "^v$escapedDate\.(\d+)-beta$"
    $existingTags = @(Invoke-Git -Arguments @("tag", "--list", "v$DateStamp.*-beta"))
    $highest = -1

    foreach ($existingTag in $existingTags) {
        if ($existingTag -match $pattern) {
            $value = [int]$Matches[1]
            if ($value -gt $highest) {
                $highest = $value
            }
        }
    }

    return "v$DateStamp.$($highest + 1)-beta"
}

function ConvertTo-CompactJson {
    param([object]$Value)

    return ($Value | ConvertTo-Json -Depth 8 -Compress)
}

function Write-GitHubOutput {
    param(
        [string]$Name,
        [string]$Value
    )

    if ([string]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) {
        return
    }

    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "$Name=$Value"
}

$repoRootPath = Resolve-RepoRoot -Value $RepoRoot
Push-Location $repoRootPath
try {
    if (-not [string]::IsNullOrWhiteSpace($Tag) -and $Tag -notmatch "^v\d{4}\.\d+\.\d+\.\d+-beta$") {
        throw "Nightly prerelease tags must match vYYYY.M.D.N-beta."
    }

    $repo = $env:GITHUB_REPOSITORY
    if ([string]::IsNullOrWhiteSpace($repo)) {
        $repo = ((Invoke-Git -Arguments @("config", "--get", "remote.origin.url")) -join "").Trim()
        $repo = $repo -replace "^https://github.com/", ""
        $repo = $repo -replace "\.git$", ""
        $repo = $repo -replace "^git@github.com:", ""
    }

    $releaseState = Read-ReleaseState -Path $ReleaseStatePath
    $nextTag = if ([string]::IsNullOrWhiteSpace($Tag)) {
        Get-NextBetaTag -DateStamp $Today
    } else {
        $Tag
    }

    $baseTag = Get-LatestReleaseTag -ReleaseState $releaseState -Repo $repo -ExcludeTag $nextTag
    $changedFiles = @(Get-ChangedFilesSinceTag -BaseTag $baseTag)
    $baseMissing = [string]::IsNullOrWhiteSpace($baseTag) -or -not (Test-GitTagExists -Name $baseTag)
    $releasablePatterns = @(Get-ReleasablePatterns)
    $releasableChanged = Test-AnyPathMatches -Files $changedFiles -Patterns $releasablePatterns

    $reasonParts = [System.Collections.Generic.List[string]]::new()
    if ($baseMissing) {
        if ([string]::IsNullOrWhiteSpace($baseTag)) {
            $reasonParts.Add("no previous release") | Out-Null
        } else {
            $reasonParts.Add("previous release tag $baseTag is not present locally") | Out-Null
        }
    }
    if ($releasableChanged) {
        $reasonParts.Add("releasable inputs changed") | Out-Null
    }
    if ($reasonParts.Count -eq 0) {
        $reasonParts.Add("unchanged since $baseTag") | Out-Null
    }

    $hasChanges = $baseMissing -or $releasableChanged
    $version = $nextTag.TrimStart("v")
    $plan = [pscustomobject]@{
        has_changes = $hasChanges
        next_tag = $nextTag
        version = $version
        base_tag = $baseTag
        changed_files = @($changedFiles)
        reason = ($reasonParts -join "; ")
    }

    $planJson = ConvertTo-CompactJson -Value $plan
    if (-not [string]::IsNullOrWhiteSpace($OutputJsonPath)) {
        $resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputJsonPath)
        $outputParent = Split-Path -Parent $resolvedOutputPath
        if (-not (Test-Path -LiteralPath $outputParent)) {
            New-Item -ItemType Directory -Path $outputParent | Out-Null
        }
        [System.IO.File]::WriteAllText($resolvedOutputPath, $planJson, (New-Object System.Text.UTF8Encoding($false)))
    }

    Write-GitHubOutput -Name "has_changes" -Value ([string]$plan.has_changes).ToLowerInvariant()
    Write-GitHubOutput -Name "next_tag" -Value $plan.next_tag
    Write-GitHubOutput -Name "version" -Value $plan.version
    Write-GitHubOutput -Name "base_tag" -Value $plan.base_tag
    Write-GitHubOutput -Name "reason" -Value $plan.reason

    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value "### Nightly prerelease plan"
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value ""
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value "- Tag: $($plan.next_tag)"
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value "- Base: $($plan.base_tag)"
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value "- Reason: $($plan.reason)"
    }

    Write-Host "Nightly prerelease plan:"
    Write-Host "  Next tag: $($plan.next_tag)"
    Write-Host "  Base tag: $($plan.base_tag)"
    Write-Host "  Has changes: $($plan.has_changes)"
    Write-Host "  Reason: $($plan.reason)"
}
finally {
    Pop-Location
}
