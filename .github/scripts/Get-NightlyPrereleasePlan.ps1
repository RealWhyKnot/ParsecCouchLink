#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string] $RepoRoot = '',
    [string] $Tag = '',
    [string] $ReleaseStatePath = '',
    [string] $Today = '',
    [datetime] $NowUtc = ([datetime]::UtcNow),
    [string] $OutputJsonPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-RepoRoot {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Value)

    if (-not [string]::IsNullOrWhiteSpace($Value)) {
        return (Resolve-Path -LiteralPath $Value).Path
    }

    return (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
}

function Invoke-Git {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([Parameter(Mandatory = $true)][string[]] $Arguments)

    $output = & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
    return [string[]]@($output)
}

function Test-GitTag {
    [CmdletBinding()]
    [OutputType([bool])]
    param([string] $Name)

    if ([string]::IsNullOrWhiteSpace($Name)) {
        return $false
    }

    $null = & {
        $ErrorActionPreference = 'Continue'
        git rev-parse --verify --quiet "refs/tags/$Name^{commit}" 2>$null
    }
    return ($LASTEXITCODE -eq 0)
}

function ConvertTo-RepoPath {
    [CmdletBinding()]
    [OutputType([string])]
    param([string] $Path)

    $normalized = $Path -replace '\\', '/'
    while ($normalized.StartsWith('./', [System.StringComparison]::Ordinal)) {
        $normalized = $normalized.Substring(2)
    }
    return $normalized
}

function Test-PathMatchesPattern {
    [CmdletBinding()]
    [OutputType([bool])]
    param(
        [string] $Path,
        [string] $Pattern
    )

    $repoPath = ConvertTo-RepoPath -Path $Path
    $repoPattern = ConvertTo-RepoPath -Path $Pattern

    if ($repoPattern.EndsWith('/', [System.StringComparison]::Ordinal)) {
        return $repoPath.StartsWith($repoPattern, [System.StringComparison]::OrdinalIgnoreCase)
    }

    return [string]::Equals($repoPath, $repoPattern, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-AnyPathMatch {
    [CmdletBinding()]
    [OutputType([bool])]
    param(
        [string[]] $Files,
        [string[]] $Patterns
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

function Get-ReleasablePattern {
    [CmdletBinding()]
    [OutputType([string[]])]
    param()

    return [string[]]@(
        '.github/scripts/',
        '.github/workflows/ci.yml',
        '.github/workflows/release.yml',
        '.github/workflows/nightly-prerelease.yml',
        '.clang-format',
        '.editorconfig',
        'build.ps1',
        'setup.ps1',
        'bridge/',
        'pico-bridge/',
        'scripts/',
        'README.md',
        'LICENSE',
        'NOTICE'
    )
}

function Read-ReleaseState {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string] $Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $null
    }

    $resolved = Resolve-Path -LiteralPath $Path
    $json = [IO.File]::ReadAllText($resolved.Path)
    if ([string]::IsNullOrWhiteSpace($json)) {
        return $null
    }

    return ($json | ConvertFrom-Json)
}

function Get-StateReleaseTag {
    [CmdletBinding()]
    [OutputType([string])]
    param([object] $ReleaseState)

    if ($null -eq $ReleaseState) {
        return ''
    }

    foreach ($name in @('latest_release_tag', 'latest_release', 'tag')) {
        $property = $ReleaseState.PSObject.Properties[$name]
        if ($null -ne $property -and $null -ne $property.Value) {
            return [string]$property.Value
        }
    }

    return ''
}

function Get-LatestReleaseTag {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [object] $ReleaseState,
        [string] $Repo,
        [string] $ExcludeTag
    )

    $stateTag = Get-StateReleaseTag -ReleaseState $ReleaseState
    if (-not [string]::IsNullOrWhiteSpace($stateTag)) {
        if ($stateTag -ne $ExcludeTag) {
            return $stateTag
        }
        return ''
    }

    $json = & gh api "repos/$Repo/releases?per_page=50"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to read releases for $Repo"
    }

    $releases = @($json | ConvertFrom-Json)
    foreach ($release in $releases) {
        $draftProperty = $release.PSObject.Properties['draft']
        $draft = $false
        if ($null -ne $draftProperty) {
            $draft = [bool]$draftProperty.Value
        }

        $tagProperty = $release.PSObject.Properties['tag_name']
        $releaseTag = ''
        if ($null -ne $tagProperty -and $null -ne $tagProperty.Value) {
            $releaseTag = [string]$tagProperty.Value
        }

        if (-not $draft -and -not [string]::IsNullOrWhiteSpace($releaseTag) -and $releaseTag -ne $ExcludeTag) {
            return $releaseTag
        }
    }

    return ''
}

function Get-ChangedFilesSinceTag {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([string] $BaseTag)

    if (-not [string]::IsNullOrWhiteSpace($BaseTag) -and (Test-GitTag -Name $BaseTag)) {
        return Invoke-Git -Arguments @('diff', '--name-only', "$BaseTag..HEAD", '--')
    }

    return Invoke-Git -Arguments @('ls-files')
}

function Get-CentralTimeZone {
    [CmdletBinding()]
    [OutputType([System.TimeZoneInfo])]
    param()

    foreach ($id in @('Central Standard Time', 'America/Chicago')) {
        try {
            return [System.TimeZoneInfo]::FindSystemTimeZoneById($id)
        } catch [System.TimeZoneNotFoundException], [System.InvalidTimeZoneException] {
            continue
        }
    }

    throw 'Could not resolve the America/Chicago release time zone.'
}

function Get-ReleaseDateStamp {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [datetime] $NowUtc,
        [string] $Format
    )

    $utc = $NowUtc
    if ($utc.Kind -ne [System.DateTimeKind]::Utc) {
        $utc = $utc.ToUniversalTime()
    }

    $central = [System.TimeZoneInfo]::ConvertTimeFromUtc($utc, (Get-CentralTimeZone))
    return $central.ToString($Format, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Get-NextBetaTag {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $DateStamp,
        [datetime] $CurrentUtc
    )

    if ([string]::IsNullOrWhiteSpace($DateStamp)) {
        $DateStamp = Get-ReleaseDateStamp -NowUtc $CurrentUtc -Format 'yyyy.M.d'
    }

    $escapedDate = [regex]::Escape($DateStamp)
    $pattern = "^v$escapedDate\.(\d+)(-[A-Za-z0-9]{4})?$"
    $existingTags = @(Invoke-Git -Arguments @('tag', '--list', "v$DateStamp.*"))
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
    [CmdletBinding()]
    [OutputType([string])]
    param([object] $Value)

    return ($Value | ConvertTo-Json -Depth 8 -Compress)
}

function Add-FileLine {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [AllowEmptyString()][string] $Line
    )

    [IO.File]::AppendAllText([IO.Path]::GetFullPath($Path), $Line + [Environment]::NewLine)
}

function Write-GitHubOutput {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Name,
        [string] $Value
    )

    if ([string]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) {
        return
    }

    Add-FileLine -Path $env:GITHUB_OUTPUT -Line "$Name=$Value"
}

$repoRootPath = Resolve-RepoRoot -Value $RepoRoot
Push-Location -LiteralPath $repoRootPath
try {
    if (-not [string]::IsNullOrWhiteSpace($Tag) -and $Tag -notmatch '^v\d{4}\.\d+\.\d+\.\d+-beta$') {
        throw 'Nightly prerelease tags must match vYYYY.M.D.N-beta.'
    }

    $repo = $env:GITHUB_REPOSITORY
    if ([string]::IsNullOrWhiteSpace($repo)) {
        $repo = ((Invoke-Git -Arguments @('config', '--get', 'remote.origin.url')) -join '').Trim()
        $repo = $repo -replace '^https://github.com/', ''
        $repo = $repo -replace '\.git$', ''
        $repo = $repo -replace '^git@github.com:', ''
    }

    $releaseState = Read-ReleaseState -Path $ReleaseStatePath
    $nextTag = if ([string]::IsNullOrWhiteSpace($Tag)) {
        Get-NextBetaTag -DateStamp $Today -CurrentUtc $NowUtc
    } else {
        $Tag
    }

    $baseTag = Get-LatestReleaseTag -ReleaseState $releaseState -Repo $repo -ExcludeTag $nextTag
    $changedFiles = @(Get-ChangedFilesSinceTag -BaseTag $baseTag)
    $baseMissing = [string]::IsNullOrWhiteSpace($baseTag) -or -not (Test-GitTag -Name $baseTag)
    $releasablePatterns = @(Get-ReleasablePattern)
    $releasableChanged = Test-AnyPathMatch -Files $changedFiles -Patterns $releasablePatterns

    $reasonParts = [System.Collections.Generic.List[string]]::new()
    if ($baseMissing) {
        if ([string]::IsNullOrWhiteSpace($baseTag)) {
            $reasonParts.Add('no previous release')
        } else {
            $reasonParts.Add("previous release tag $baseTag is not present locally")
        }
    }
    if ($releasableChanged) {
        $reasonParts.Add('releasable inputs changed')
    }
    if ($reasonParts.Count -eq 0) {
        $reasonParts.Add("unchanged since $baseTag")
    }

    $hasChanges = $baseMissing -or $releasableChanged
    $version = $nextTag.TrimStart('v')
    $plan = [pscustomobject]@{
        has_changes   = $hasChanges
        next_tag      = $nextTag
        version       = $version
        base_tag      = $baseTag
        changed_files = @($changedFiles)
        reason        = ($reasonParts -join '; ')
    }

    $planJson = ConvertTo-CompactJson -Value $plan
    if (-not [string]::IsNullOrWhiteSpace($OutputJsonPath)) {
        $resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputJsonPath)
        $outputParent = Split-Path -Parent $resolvedOutputPath
        if (-not (Test-Path -LiteralPath $outputParent)) {
            $null = New-Item -ItemType Directory -Path $outputParent
        }
        [System.IO.File]::WriteAllText($resolvedOutputPath, $planJson, (New-Object System.Text.UTF8Encoding($false)))
    }

    Write-GitHubOutput -Name 'has_changes' -Value ([string]$plan.has_changes).ToLowerInvariant()
    Write-GitHubOutput -Name 'next_tag' -Value $plan.next_tag
    Write-GitHubOutput -Name 'version' -Value $plan.version
    Write-GitHubOutput -Name 'base_tag' -Value $plan.base_tag
    Write-GitHubOutput -Name 'reason' -Value $plan.reason

    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
        Add-FileLine -Path $env:GITHUB_STEP_SUMMARY -Line '### Nightly prerelease plan'
        Add-FileLine -Path $env:GITHUB_STEP_SUMMARY -Line ''
        Add-FileLine -Path $env:GITHUB_STEP_SUMMARY -Line "- Tag: $($plan.next_tag)"
        Add-FileLine -Path $env:GITHUB_STEP_SUMMARY -Line "- Base: $($plan.base_tag)"
        Add-FileLine -Path $env:GITHUB_STEP_SUMMARY -Line "- Reason: $($plan.reason)"
    }

    Write-Host 'Nightly prerelease plan:'
    Write-Host "  Next tag: $($plan.next_tag)"
    Write-Host "  Base tag: $($plan.base_tag)"
    Write-Host "  Has changes: $($plan.has_changes)"
    Write-Host "  Reason: $($plan.reason)"
} finally {
    Pop-Location
}

exit 0
