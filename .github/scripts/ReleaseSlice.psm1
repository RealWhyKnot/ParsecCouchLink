$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-NativeQuiet {
    [CmdletBinding()]
    [OutputType([string[]])]
    param(
        [Parameter(Mandatory = $true)][string] $Command,
        [string[]] $Arguments
    )

    $run = {
        param([string] $Name, [string[]] $List)

        $ErrorActionPreference = 'Continue'
        & $Name @List 2>$null
    }
    & $run -Name $Command -List $Arguments
}

function Get-KnownPrereleaseTag {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [string] $Repo,
        [string[]] $AdditionalTags
    )

    $set = @{}
    foreach ($tagName in $AdditionalTags) {
        if ($tagName) { $set[$tagName] = $true }
    }
    if (-not $Repo) { return $set }

    $listArgs = @('release', 'list', '--repo', $Repo, '--limit', '100', '--json', 'tagName,isPrerelease')
    $listJson = Invoke-NativeQuiet -Command 'gh' -Arguments $listArgs
    if ($LASTEXITCODE -ne 0 -or -not $listJson) { return $set }

    try {
        $releases = $listJson | ConvertFrom-Json
        foreach ($release in $releases) {
            if ($release.isPrerelease -and $release.tagName) {
                $set[$release.tagName] = $true
            }
        }
    } catch [System.ArgumentException] {
        Write-Host "::warning::Failed to parse 'gh release list' output while reading prerelease tags: $_."
    }

    return $set
}

function Test-IsPrereleaseTag {
    [CmdletBinding()]
    [OutputType([bool])]
    param(
        [string] $Tag,
        [hashtable] $KnownPrereleaseTags
    )

    if ($KnownPrereleaseTags -and $KnownPrereleaseTags.ContainsKey($Tag)) { return $true }
    return $Tag -match '^v?\d{4}\.\d+\.\d+\.\d+-.+'
}

function New-SliceInfo {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [string] $Tag,
        [string[]] $LogArgs = @(),
        [string] $Display,
        [string] $Source
    )

    return @{
        Tag     = $Tag
        LogArgs = $LogArgs
        Display = $Display
        Source  = $Source
    }
}

function Find-CommitBySubject {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $Tag,
        [string] $Subject
    )

    $logLines = Invoke-NativeQuiet -Command 'git' -Arguments @('log', $Tag, '--format=%H%x09%s')
    if ($LASTEXITCODE -ne 0 -or -not $logLines) { return $null }

    foreach ($line in @($logLines)) {
        if (-not $line) { continue }
        $parts = $line -split "`t", 2
        if ($parts.Count -eq 2 -and $parts[1] -eq $Subject) {
            return $parts[0]
        }
    }
    return $null
}

function Get-PublishedStableTag {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $Tag,
        [string[]] $ListJson
    )

    try {
        $releases = $ListJson | ConvertFrom-Json
        $candidate = $releases |
            Where-Object { $_.tagName -ne $Tag -and -not $_.isPrerelease } |
            Sort-Object publishedAt -Descending |
            Select-Object -First 1
        if ($candidate) { return $candidate.tagName }
    } catch [System.ArgumentException] {
        Write-Host "::warning::Failed to parse 'gh release list' output: $_."
    }
    return $null
}

function Resolve-PublishedSlice {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [string] $Tag,
        [string] $Repo
    )

    $listArgs = @('release', 'list', '--repo', $Repo, '--limit', '20', '--json', 'tagName,publishedAt,isPrerelease')
    $listJson = Invoke-NativeQuiet -Command 'gh' -Arguments $listArgs
    if ($LASTEXITCODE -ne 0 -or -not $listJson) {
        $warning = @(
            "::warning::'gh release list' produced no usable output (gh not authed or no releases yet).",
            'Falling back to first-release changelog notes.'
        ) -join ' '
        Write-Host $warning
        return $null
    }

    $candidatePrevTag = Get-PublishedStableTag -Tag $Tag -ListJson $listJson
    if (-not $candidatePrevTag) { return $null }

    $orphanSha = Invoke-NativeQuiet -Command 'git' -Arguments @('rev-list', '-n', '1', $candidatePrevTag)
    if ($LASTEXITCODE -ne 0 -or -not $orphanSha) { return $null }
    $orphanSha = $orphanSha.Trim()

    $null = Invoke-NativeQuiet -Command 'git' -Arguments @('merge-base', '--is-ancestor', $orphanSha, $Tag)
    if ($LASTEXITCODE -eq 0) {
        $warning = @(
            "::warning::Using published stable release $candidatePrevTag as slice base",
            'after describe exceeded the sanity cap.'
        ) -join ' '
        Write-Host $warning
        $slice = "$candidatePrevTag..$Tag"
        return New-SliceInfo -Tag $candidatePrevTag -LogArgs @($slice) -Display $slice -Source 'published-release'
    }

    $orphanSubject = Invoke-NativeQuiet -Command 'git' -Arguments @('show', '-s', '--format=%s', $orphanSha)
    if ($LASTEXITCODE -ne 0 -or -not $orphanSubject) { return $null }

    $rebasedSha = Find-CommitBySubject -Tag $Tag -Subject $orphanSubject
    if ($rebasedSha) {
        $shortSha = $rebasedSha.Substring(0, 12)
        $warning = @(
            "::warning::Subject-matched slice: prev tag $candidatePrevTag",
            "(orphan sha $($orphanSha.Substring(0,12))) matches current-history sha $shortSha by subject;",
            "using $shortSha..$Tag."
        ) -join ' '
        Write-Host $warning
        $display = "$candidatePrevTag..$Tag (subject-matched at $shortSha)"
        $logArgs = @("$rebasedSha..$Tag")
        return New-SliceInfo -Tag $candidatePrevTag -LogArgs $logArgs -Display $display -Source 'subject-match'
    }

    $warning = @(
        "::warning::Prev tag $candidatePrevTag subject '$orphanSubject' not found in current $Tag history.",
        'Falling back to first-release changelog notes.'
    ) -join ' '
    Write-Host $warning
    return $null
}

function Resolve-PrevTagForSlice {
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [string] $Tag,
        [string] $Repo,
        [string[]] $AdditionalPrereleaseTags
    )

    $knownPrereleaseTags = Get-KnownPrereleaseTag -Repo $Repo -AdditionalTags $AdditionalPrereleaseTags

    $describeArgs = [System.Collections.Generic.List[string]]::new()
    $describeArgs.AddRange([string[]]@('describe', '--tags', '--abbrev=0'))
    if (-not (Test-IsPrereleaseTag -Tag $Tag -KnownPrereleaseTags $knownPrereleaseTags)) {
        $describeArgs.AddRange([string[]]@('--exclude', '*-*'))
        foreach ($preTag in $knownPrereleaseTags.Keys) {
            if ($preTag -and $preTag -ne $Tag) {
                $describeArgs.AddRange([string[]]@('--exclude', $preTag))
            }
        }
    }
    $describeArgs.Add("$Tag^")

    $prevRef = Invoke-NativeQuiet -Command 'git' -Arguments $describeArgs.ToArray()
    if ($LASTEXITCODE -eq 0 -and $prevRef) {
        $prevTag = $prevRef.Trim()
        $count = Invoke-NativeQuiet -Command 'git' -Arguments @('rev-list', '--count', "$prevTag..$Tag")
        if ($LASTEXITCODE -eq 0 -and [int]$count -le 50) {
            $slice = "$prevTag..$Tag"
            return New-SliceInfo -Tag $prevTag -LogArgs @($slice) -Display $slice -Source 'describe'
        }
        $warning = @(
            "::warning::Slice from $prevTag..$Tag is $count commits (>50 cap).",
            'Falling back to subject-match against the most recent published release.'
        ) -join ' '
        Write-Host $warning
    }

    if ($Repo) {
        $published = Resolve-PublishedSlice -Tag $Tag -Repo $Repo
        if ($published) { return $published }
    }

    Write-Host "::warning::No prior tag or published release matched; treating $Tag as the first release."
    return New-SliceInfo -Tag '' -Display "$Tag (first release)" -Source 'first-release'
}

Export-ModuleMember -Function Invoke-NativeQuiet, Resolve-PrevTagForSlice
