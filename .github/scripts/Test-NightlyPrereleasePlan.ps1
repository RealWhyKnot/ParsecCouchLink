#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$Planner = Join-Path $ScriptRoot "Get-NightlyPrereleasePlan.ps1"

function Invoke-TestGit {
    param(
        [string]$RepoRoot,
        [string[]]$Arguments
    )

    Push-Location $RepoRoot
    try {
        $output = & git @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
        }
        return @($output)
    }
    finally {
        Pop-Location
    }
}

function Write-TestFile {
    param(
        [string]$Path,
        [string]$Content
    )

    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($parent) -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent | Out-Null
    }

    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function New-TestRepo {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ("couchlink-nightly-prerelease-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $root | Out-Null

    Invoke-TestGit -RepoRoot $root -Arguments @("init", "-q", ".") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("config", "user.name", "CouchLink Tests") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("config", "user.email", "couchlink-tests@example.invalid") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("config", "core.autocrlf", "false") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("remote", "add", "origin", "https://github.com/owner/repo.git") | Out-Null

    Write-TestFile -Path (Join-Path $root "build.ps1") -Content "param()`n"
    Write-TestFile -Path (Join-Path $root "setup.ps1") -Content "Write-Host setup`n"
    Write-TestFile -Path (Join-Path $root "bridge/src/main.rs") -Content "fn main() {}`n"
    Write-TestFile -Path (Join-Path $root "pico-bridge/src/main.c") -Content "int main(void) { return 0; }`n"
    Write-TestFile -Path (Join-Path $root "CHANGELOG.md") -Content "# Changelog`n"
    Write-TestFile -Path (Join-Path $root "wiki/Quick-Start.md") -Content "# Quick Start`n"

    Invoke-TestGit -RepoRoot $root -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("commit", "-q", "-m", "initial") | Out-Null
    Invoke-TestGit -RepoRoot $root -Arguments @("tag", "v2026.6.1.0") | Out-Null

    return $root
}

function Write-ReleaseState {
    param(
        [string]$RepoRoot,
        [string]$Tag
    )

    $state = [ordered]@{
        latest_release_tag = $Tag
    }
    $path = Join-Path $RepoRoot "release-state.json"
    [System.IO.File]::WriteAllText($path, ($state | ConvertTo-Json -Depth 4), (New-Object System.Text.UTF8Encoding($false)))
    return $path
}

function Invoke-Plan {
    param(
        [string]$RepoRoot,
        [string]$ReleaseStatePath,
        [string]$Tag = "",
        [string]$Today = ""
    )

    $outputPath = Join-Path $RepoRoot "plan.json"
    $arguments = @{
        RepoRoot = $RepoRoot
        ReleaseStatePath = $ReleaseStatePath
        OutputJsonPath = $outputPath
    }
    if (-not [string]::IsNullOrWhiteSpace($Tag)) {
        $arguments["Tag"] = $Tag
    }
    if (-not [string]::IsNullOrWhiteSpace($Today)) {
        $arguments["Today"] = $Today
    }

    & $Planner @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Planner failed with exit code $LASTEXITCODE"
    }

    return (Get-Content -LiteralPath $outputPath -Raw | ConvertFrom-Json)
}

function Assert-Equal {
    param(
        [object]$Actual,
        [object]$Expected,
        [string]$Message
    )

    if ($Actual -ne $Expected) {
        throw "$Message. Expected '$Expected', got '$Actual'."
    }
}

$tempRoots = [System.Collections.Generic.List[string]]::new()
try {
    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Tag "v2026.6.2.0-beta"
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message "No changes should not produce a prerelease plan"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    Write-TestFile -Path (Join-Path $repo "bridge/src/main.rs") -Content "fn main() { println!(`"hi`"); }`n"
    Invoke-TestGit -RepoRoot $repo -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("commit", "-q", "-m", "change bridge") | Out-Null
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Tag "v2026.6.2.0-beta"
    Assert-Equal -Actual $plan.has_changes -Expected $true -Message "Bridge changes should produce a prerelease plan"
    Assert-Equal -Actual $plan.version -Expected "2026.6.2.0-beta" -Message "Planner should expose the bare release version"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    Write-TestFile -Path (Join-Path $repo "CHANGELOG.md") -Content "# Changelog`n`nchanged`n"
    Invoke-TestGit -RepoRoot $repo -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("commit", "-q", "-m", "docs changelog") | Out-Null
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Tag "v2026.6.2.0-beta"
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message "Changelog-only changes should not produce a prerelease plan"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    Write-TestFile -Path (Join-Path $repo "wiki/Quick-Start.md") -Content "# Updated docs`n"
    Invoke-TestGit -RepoRoot $repo -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("commit", "-q", "-m", "docs wiki") | Out-Null
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Tag "v2026.6.2.0-beta"
    Assert-Equal -Actual $plan.has_changes -Expected $false -Message "Wiki-only changes should not produce a prerelease plan"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    Invoke-TestGit -RepoRoot $repo -Arguments @("tag", "v2026.6.9.0-beta") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("tag", "v2026.6.9.1-beta") | Out-Null
    Write-TestFile -Path (Join-Path $repo "pico-bridge/src/main.c") -Content "int main(void) { return 1; }`n"
    Invoke-TestGit -RepoRoot $repo -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("commit", "-q", "-m", "change firmware") | Out-Null
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Today "2026.6.9"
    Assert-Equal -Actual $plan.next_tag -Expected "v2026.6.9.2-beta" -Message "Next beta tag should increment the same-day sequence"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    Invoke-TestGit -RepoRoot $repo -Arguments @("tag", "v2026.6.9.0") | Out-Null
    Write-TestFile -Path (Join-Path $repo "bridge/src/main.rs") -Content "fn main() { println!(`"stable today`"); }`n"
    Invoke-TestGit -RepoRoot $repo -Arguments @("add", ".") | Out-Null
    Invoke-TestGit -RepoRoot $repo -Arguments @("commit", "-q", "-m", "change after same-day stable release") | Out-Null
    $plan = Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Today "2026.6.9"
    Assert-Equal -Actual $plan.next_tag -Expected "v2026.6.9.1-beta" -Message "Next beta tag should increment after a same-day stable release"

    $repo = New-TestRepo
    $tempRoots.Add($repo) | Out-Null
    $state = Write-ReleaseState -RepoRoot $repo -Tag "v2026.6.1.0"
    $failed = $false
    try {
        Invoke-Plan -RepoRoot $repo -ReleaseStatePath $state -Tag "v2026.6.2.0-beta.1" | Out-Null
    }
    catch {
        $failed = $true
    }
    Assert-Equal -Actual $failed -Expected $true -Message "Planner should reject numbered beta suffixes"

    Write-Host "Nightly prerelease planner tests passed."
}
finally {
    foreach ($tempRoot in $tempRoots) {
        if (Test-Path -LiteralPath $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }
}
