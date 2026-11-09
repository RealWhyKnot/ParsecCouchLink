#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-TestGit {
    [CmdletBinding()]
    [OutputType([string[]])]
    param(
        [string] $RepoRoot,
        [string[]] $Arguments
    )

    Push-Location -LiteralPath $RepoRoot
    try {
        $output = & git @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
        }
        return [string[]]@($output)
    } finally {
        Pop-Location
    }
}

function Write-TestFile {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $Path,
        [string] $Content
    )

    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($parent) -and -not (Test-Path -LiteralPath $parent)) {
        $null = New-Item -ItemType Directory -Path $parent
    }

    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function New-TestRepo {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    $name = 'couchlink-nightly-prerelease-' + [System.Guid]::NewGuid().ToString('N')
    $root = Join-Path ([System.IO.Path]::GetTempPath()) $name
    $null = New-Item -ItemType Directory -Path $root

    $origin = 'https://github.com/owner/repo.git'
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('init', '-q', '.')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('config', 'user.name', 'CouchLink Tests')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('config', 'user.email', 'couchlink-tests@example.invalid')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('config', 'core.autocrlf', 'false')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('remote', 'add', 'origin', $origin)

    Write-TestFile -Path (Join-Path $root 'build.ps1') -Content "param()`n"
    Write-TestFile -Path (Join-Path $root 'setup.ps1') -Content "Write-Host setup`n"
    Write-TestFile -Path (Join-Path $root 'bridge/src/main.rs') -Content "fn main() {}`n"
    Write-TestFile -Path (Join-Path $root 'pico-bridge/src/main.c') -Content "int main(void) { return 0; }`n"
    Write-TestFile -Path (Join-Path $root 'CHANGELOG.md') -Content "# Changelog`n"
    Write-TestFile -Path (Join-Path $root 'docs/notes.md') -Content "# Notes`n"

    $null = Invoke-TestGit -RepoRoot $root -Arguments @('add', '.')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('commit', '-q', '-m', 'initial')
    $null = Invoke-TestGit -RepoRoot $root -Arguments @('tag', 'v2026.6.1.0')

    return $root
}

function Write-ReleaseState {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [string] $RepoRoot,
        [string] $Tag
    )

    $state = [ordered]@{
        latest_release_tag = $Tag
    }
    $path = Join-Path $RepoRoot 'release-state.json'
    $json = $state | ConvertTo-Json -Depth 4
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    return $path
}

function Invoke-Plan {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [string] $Planner,
        [string] $RepoRoot,
        [string] $ReleaseStatePath,
        [string] $Tag = '',
        [string] $Today = '',
        [string] $NowUtc = ''
    )

    $outputPath = Join-Path $RepoRoot 'plan.json'
    $arguments = @{
        RepoRoot         = $RepoRoot
        ReleaseStatePath = $ReleaseStatePath
        OutputJsonPath   = $outputPath
    }
    if (-not [string]::IsNullOrWhiteSpace($Tag)) {
        $arguments['Tag'] = $Tag
    }
    if (-not [string]::IsNullOrWhiteSpace($Today)) {
        $arguments['Today'] = $Today
    }
    if (-not [string]::IsNullOrWhiteSpace($NowUtc)) {
        $arguments['NowUtc'] = [datetime]::Parse(
            $NowUtc,
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::AdjustToUniversal
        )
    }

    & $Planner @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Planner failed with exit code $LASTEXITCODE"
    }

    return ([IO.File]::ReadAllText($outputPath) | ConvertFrom-Json)
}

function Assert-Equal {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [object] $Actual,
        [object] $Expected,
        [string] $Message
    )

    if ($Actual -ne $Expected) {
        throw "$Message. Expected '$Expected', got '$Actual'."
    }
}

function Add-TestCommit {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [string] $RepoRoot,
        [string] $Subject
    )

    $null = Invoke-TestGit -RepoRoot $RepoRoot -Arguments @('add', '.')
    $null = Invoke-TestGit -RepoRoot $RepoRoot -Arguments @('commit', '-q', '-m', $Subject)
}

$planner = Join-Path $PSScriptRoot 'Get-NightlyPrereleasePlan.ps1'
$tempRoots = [System.Collections.Generic.List[string]]::new()
try {
    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Tag 'v2026.6.2.0-beta'
    $message = 'No changes should not produce a prerelease plan'
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    Write-TestFile -Path (Join-Path $repo 'bridge/src/main.rs') -Content "fn main() { println!(`"hi`"); }`n"
    Add-TestCommit -RepoRoot $repo -Subject 'change bridge'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Tag 'v2026.6.2.0-beta'
    $message = 'Bridge changes should produce a prerelease plan'
    Assert-Equal -Actual $plan.has_changes -Expected $true -Message $message
    $message = 'Planner should expose the bare release version'
    Assert-Equal -Actual $plan.version -Expected '2026.6.2.0-beta' -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    Write-TestFile -Path (Join-Path $repo 'CHANGELOG.md') -Content "# Changelog`n`nchanged`n"
    Add-TestCommit -RepoRoot $repo -Subject 'docs changelog'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Tag 'v2026.6.2.0-beta'
    $message = 'Changelog-only changes should not produce a prerelease plan'
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    Write-TestFile -Path (Join-Path $repo 'docs/notes.md') -Content "# Updated docs`n"
    Add-TestCommit -RepoRoot $repo -Subject 'docs notes'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Tag 'v2026.6.2.0-beta'
    $message = 'Docs-only changes should not produce a prerelease plan'
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    $null = Invoke-TestGit -RepoRoot $repo -Arguments @('tag', 'v2026.6.9.0-beta')
    $null = Invoke-TestGit -RepoRoot $repo -Arguments @('tag', 'v2026.6.9.1-beta')
    Write-TestFile -Path (Join-Path $repo 'pico-bridge/src/main.c') -Content "int main(void) { return 1; }`n"
    Add-TestCommit -RepoRoot $repo -Subject 'change firmware'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Today '2026.6.9'
    $message = 'Next beta tag should increment the same-day sequence'
    Assert-Equal -Actual $plan.next_tag -Expected 'v2026.6.9.2-beta' -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    $null = Invoke-TestGit -RepoRoot $repo -Arguments @('tag', 'v2026.6.9.0')
    $source = "fn main() { println!(`"stable today`"); }`n"
    Write-TestFile -Path (Join-Path $repo 'bridge/src/main.rs') -Content $source
    Add-TestCommit -RepoRoot $repo -Subject 'change after same-day stable release'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Today '2026.6.9'
    $message = 'Next beta tag should increment after a same-day stable release'
    Assert-Equal -Actual $plan.next_tag -Expected 'v2026.6.9.1-beta' -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    $source = "fn main() { println!(`"central date`"); }`n"
    Write-TestFile -Path (Join-Path $repo 'bridge/src/main.rs') -Content $source
    Add-TestCommit -RepoRoot $repo -Subject 'change during utc rollover'
    $plan = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -NowUtc '2026-06-16T01:30:00Z'
    $message = 'Default prerelease date should use America/Chicago, not UTC'
    Assert-Equal -Actual $plan.next_tag -Expected 'v2026.6.15.0-beta' -Message $message

    $repo = New-TestRepo
    $tempRoots.Add($repo)
    $state = Write-ReleaseState -RepoRoot $repo -Tag 'v2026.6.1.0'
    $failed = $false
    try {
        $null = Invoke-Plan -Planner $planner -RepoRoot $repo -ReleaseStatePath $state -Tag 'v2026.6.2.0-beta.1'
    } catch [System.Management.Automation.RuntimeException] {
        $failed = $true
    }
    Assert-Equal -Actual $failed -Expected $true -Message 'Planner should reject numbered beta suffixes'

    Write-Host 'Nightly prerelease planner tests passed.'
} finally {
    foreach ($tempRoot in $tempRoots) {
        if (Test-Path -LiteralPath $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }
}

exit 0
