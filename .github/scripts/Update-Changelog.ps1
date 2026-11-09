#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Append', 'Promote', 'Notes')]
    [string] $Mode,

    [string] $Range,
    [string] $Version,
    [switch] $ForVersion,
    [string] $Repo = $env:GITHUB_REPOSITORY,
    [string] $RepoRoot,
    [datetime] $NowUtc = ([datetime]::UtcNow)
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function New-Utf8NoBomEncoding {
    [CmdletBinding()]
    [OutputType([System.Text.UTF8Encoding])]
    param()

    return New-Object System.Text.UTF8Encoding -ArgumentList $false
}

function Read-TextFile {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory = $true)][string] $Path)

    return [System.IO.File]::ReadAllText($Path, (New-Utf8NoBomEncoding))
}

function Write-TextFile {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][string] $Content
    )

    [System.IO.File]::WriteAllText($Path, $Content, (New-Utf8NoBomEncoding))
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

function Remove-BuildStamp {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory = $true)][string] $Subject)

    return ($Subject -replace ' \(\d{4}\.\d+\.\d+\.\d+(-[A-Za-z0-9]{4})?\)$', '').Trim()
}

function ConvertTo-ChangelogEntry {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [Parameter(Mandatory = $true)][string] $Sha,
        [Parameter(Mandatory = $true)][string] $Subject
    )

    $clean = Remove-BuildStamp -Subject $Subject
    if ([string]::IsNullOrWhiteSpace($clean)) { return $null }
    if ($clean -match '\[skip changelog\]') { return $null }
    if ($clean -match '^Merge ') { return $null }

    $typePattern = '^(?<type>feat|fix|perf|refactor|style|revert|docs|build|ci|chore|test)'
    $restPattern = '(?:\((?<scope>[^)]+)\))?(?<bang>!)?:\s+(?<desc>.+)$'
    $m = [regex]::Match($clean, $typePattern + $restPattern)
    if (-not $m.Success) {
        return @{
            Bucket = 'Changed'
            Bullet = "- $clean (" + $Sha.Substring(0, 7) + ')'
        }
    }

    $type = $m.Groups['type'].Value
    $scope = $m.Groups['scope'].Value
    $desc = $m.Groups['desc'].Value
    $isBreaking = $m.Groups['bang'].Success

    if ($desc.Length -gt 0) {
        $desc = $desc.Substring(0, 1).ToUpperInvariant() + $desc.Substring(1)
    }

    $bucket = switch ($type) {
        'feat' { 'Added' }
        'fix' { 'Fixed' }
        'perf' { 'Changed' }
        'refactor' { 'Changed' }
        'style' { 'Changed' }
        'revert' { 'Changed' }
        'chore' {
            if ($scope -and $scope -match '^deps') { 'Changed' } else { $null }
        }
        default { $null }
    }

    if (-not $bucket) { return $null }
    if ($isBreaking) { $bucket = 'Breaking' }

    $prefix = if ($scope) { "**${scope}:** " } else { '' }
    return @{
        Bucket = $bucket
        Bullet = "- $prefix$desc (" + $Sha.Substring(0, 7) + ')'
    }
}

function Find-UnreleasedSection {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Parameter(Mandatory = $true)][string] $Content)

    $lines = $Content -split '\r?\n'
    $startIdx = -1
    for ($i = 0; $i -lt $lines.Length; $i++) {
        if ($lines[$i] -match '^##\s+Unreleased\s*$') {
            $startIdx = $i
            break
        }
    }
    if ($startIdx -lt 0) { return $null }

    $endIdx = $lines.Length
    for ($j = $startIdx + 1; $j -lt $lines.Length; $j++) {
        if ($lines[$j] -match '^---\s*$' -or $lines[$j] -match '^##\s+') {
            $endIdx = $j
            break
        }
    }

    return @{
        Lines    = $lines
        StartIdx = $startIdx
        EndIdx   = $endIdx
    }
}

function Get-SectionBody {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([Parameter(Mandatory = $true)][hashtable] $Section)

    $bodyStart = $Section['StartIdx'] + 1
    $bodyEnd = $Section['EndIdx'] - 1
    if ($bodyEnd -ge $bodyStart) {
        return [string[]]$Section['Lines'][$bodyStart..$bodyEnd]
    }
    return [string[]]@()
}

function Get-SectionTail {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([Parameter(Mandatory = $true)][hashtable] $Section)

    $lines = $Section['Lines']
    if ($Section['EndIdx'] -lt $lines.Length) {
        return [string[]]$lines[$Section['EndIdx']..($lines.Length - 1)]
    }
    return [string[]]@()
}

function Read-BucketTable {
    [CmdletBinding()]
    [OutputType([System.Collections.Specialized.OrderedDictionary])]
    param([string[]] $BodyLines)

    $buckets = [ordered]@{}
    $current = $null
    foreach ($line in $BodyLines) {
        if ($line -match '^###\s+(?<name>.+?)\s*$') {
            $current = $matches['name']
            if (-not $buckets.Contains($current)) {
                $buckets[$current] = [System.Collections.Generic.List[string]]::new()
            }
            continue
        }
        if ($line -match '^\s*- ' -and $current) {
            $buckets[$current].Add($line)
        }
    }
    return $buckets
}

function Format-BucketTable {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([System.Collections.IDictionary] $Buckets)

    $hasEntries = $false
    foreach ($key in $Buckets.Keys) {
        if ($Buckets[$key].Count -gt 0) {
            $hasEntries = $true
            break
        }
    }
    if (-not $hasEntries) {
        return [string[]]@('', '_No notable changes since the last release._', '')
    }

    $out = [System.Collections.Generic.List[string]]::new()
    $out.Add('')
    $emitted = @{}
    foreach ($name in @('Breaking', 'Added', 'Changed', 'Fixed')) {
        if ($Buckets.Contains($name) -and $Buckets[$name].Count -gt 0) {
            $out.Add("### $name")
            $out.AddRange([string[]]@($Buckets[$name]))
            $out.Add('')
            $emitted[$name] = $true
        }
    }
    foreach ($name in $Buckets.Keys) {
        if (-not $emitted.Contains($name) -and $Buckets[$name].Count -gt 0) {
            $out.Add("### $name")
            $out.AddRange([string[]]@($Buckets[$name]))
            $out.Add('')
        }
    }
    return $out.ToArray()
}

function Update-Unreleased {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $ChangelogPath,
        [hashtable] $NewEntries
    )

    $content = Read-TextFile -Path $ChangelogPath
    $section = Find-UnreleasedSection -Content $content
    if (-not $section) {
        throw "CHANGELOG.md is missing the '## Unreleased' section."
    }

    $lines = $section['Lines']
    $buckets = Read-BucketTable -BodyLines (Get-SectionBody -Section $section)

    foreach ($bucket in $NewEntries.Keys) {
        if (-not $buckets.Contains($bucket)) {
            $buckets[$bucket] = [System.Collections.Generic.List[string]]::new()
        }
        foreach ($bullet in $NewEntries[$bucket]) {
            $shortSha = if ($bullet -match '\(([a-f0-9]{7})\)\s*$') { $matches[1] } else { $null }
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
                $buckets[$bucket].Add($bullet)
            }
        }
    }

    $before = if ($section['StartIdx'] -gt 0) { $lines[0..($section['StartIdx'])] } else { @($lines[0]) }
    $newLines = [System.Collections.Generic.List[string]]::new()
    $newLines.AddRange([string[]]@($before))
    $newLines.AddRange([string[]]@(Format-BucketTable -Buckets $buckets))
    $newLines.AddRange([string[]]@(Get-SectionTail -Section $section))

    Write-TextFile -Path $ChangelogPath -Content ($newLines -join "`n")
}

function Invoke-AppendMode {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $RepoRoot,
        [Parameter(Mandatory = $true)][string] $ChangelogPath,
        [string] $Range
    )

    if (-not $Range) { throw 'Append mode requires -Range.' }

    Push-Location -LiteralPath $RepoRoot
    try {
        $log = & {
            $ErrorActionPreference = 'Continue'
            git log --no-merges --format='%H%x09%s%x09%ae' $Range 2>$null
        }
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
    foreach ($line in ($log -split '\r?\n')) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $parts = $line -split "`t", 3
        if ($parts.Length -lt 3) { continue }
        $sha = $parts[0]
        $subject = $parts[1]
        $email = $parts[2]
        $considered++
        if ($email -match 'github-actions\[bot\]' -or $email -match 'noreply@github.com') { continue }

        $entry = ConvertTo-ChangelogEntry -Sha $sha -Subject $subject
        if (-not $entry) { continue }

        $bucket = $entry['Bucket']
        if (-not $newEntries.ContainsKey($bucket)) {
            $newEntries[$bucket] = [System.Collections.Generic.List[string]]::new()
        }
        $newEntries[$bucket].Add($entry['Bullet'])
        $included++
    }

    Write-Host "Considered $considered commit(s), included $included."
    if ($included -eq 0) { return }

    Update-Unreleased -ChangelogPath $ChangelogPath -NewEntries $newEntries
    Write-Host 'Updated CHANGELOG.md.'
}

function Invoke-PromoteMode {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)][string] $ChangelogPath,
        [string] $Version,
        [string] $Repo,
        [datetime] $NowUtc
    )

    if (-not $Version) { throw 'Promote mode requires -Version.' }
    if (-not $Repo) { throw 'Promote mode requires -Repo or GITHUB_REPOSITORY.' }

    $today = Get-ReleaseDateStamp -NowUtc $NowUtc -Format 'yyyy-MM-dd'
    $heading = "## [$Version](https://github.com/$Repo/releases/tag/$Version) -- $today"
    $content = Read-TextFile -Path $ChangelogPath
    $section = Find-UnreleasedSection -Content $content
    if (-not $section) {
        throw "CHANGELOG.md is missing the '## Unreleased' section."
    }

    $lines = $section['Lines']
    $bodyLines = Get-SectionBody -Section $section

    $hasRealEntry = $false
    foreach ($line in $bodyLines) {
        if ($line -match '^\s*- ' -or $line -match '^###\s+') {
            $hasRealEntry = $true
            break
        }
    }
    if (-not $hasRealEntry) {
        $bodyLines = @('', '_Maintenance release; see commit log for details._', '')
    }

    $before = if ($section['StartIdx'] -gt 0) { $lines[0..($section['StartIdx'] - 1)] } else { @() }
    $newLines = [System.Collections.Generic.List[string]]::new()
    $newLines.AddRange([string[]]@($before))
    $newLines.AddRange([string[]]@(
            '## Unreleased',
            '',
            '_No notable changes since the last release._',
            '',
            '---',
            '',
            $heading
        ))
    $newLines.AddRange([string[]]@($bodyLines))
    $newLines.AddRange([string[]]@(Get-SectionTail -Section $section))

    Write-TextFile -Path $ChangelogPath -Content ($newLines -join "`n")
    Write-Host "Promoted Unreleased to $Version."
}

function Get-ReleaseNote {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory = $true)][string] $ChangelogPath,
        [string] $Version,
        [switch] $ForVersion
    )

    $content = Read-TextFile -Path $ChangelogPath
    if ($ForVersion) {
        if (-not $Version) { throw 'Notes -ForVersion requires -Version.' }
        $escaped = [regex]::Escape($Version)
        $pattern = '(?ms)^##\s+\[' + $escaped + '\][^\n]*\n(.*?)(?=^---\s*$|^##\s+|\z)'
        $match = [regex]::Match($content, $pattern)
        if (-not $match.Success) {
            throw "No changelog section found for $Version."
        }
        return $match.Groups[1].Value.Trim()
    }

    $section = Find-UnreleasedSection -Content $content
    if (-not $section) { throw "No '## Unreleased' section found." }
    return ((Get-SectionBody -Section $section) -join "`n").Trim()
}

if (-not $RepoRoot) {
    $RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
}

$changelogPath = Join-Path $RepoRoot 'CHANGELOG.md'
if (-not (Test-Path -LiteralPath $changelogPath)) {
    throw "CHANGELOG.md not found at $changelogPath"
}

switch ($Mode) {
    'Append' { Invoke-AppendMode -RepoRoot $RepoRoot -ChangelogPath $changelogPath -Range $Range }
    'Promote' { Invoke-PromoteMode -ChangelogPath $changelogPath -Version $Version -Repo $Repo -NowUtc $NowUtc }
    'Notes' { Get-ReleaseNote -ChangelogPath $changelogPath -Version $Version -ForVersion:$ForVersion }
}

exit 0
