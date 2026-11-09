#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch] $Fix,
    [ValidateSet('Repo', 'PowerShell', 'ScriptTests', 'Rust', 'Python', 'Shell', 'Firmware')]
    [string[]] $Section = @('Repo', 'PowerShell', 'ScriptTests', 'Rust', 'Python', 'Shell', 'Firmware')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Native {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [Parameter(Mandatory = $true)]
        [string] $FilePath,
        [string[]] $Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Invoke-Uv {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory = $true)][string[]] $Arguments)

    $uvArgs = [System.Collections.Generic.List[string]]::new()
    $uvArgs.AddRange([string[]]@('run', '--frozen'))
    $uvArgs.AddRange($Arguments)
    Invoke-Native -FilePath 'uv' -Arguments $uvArgs.ToArray()
}

function Get-TrackedFile {
    [CmdletBinding()]
    [OutputType([string[]])]
    param([Parameter(Mandatory = $true)][string[]] $Pattern)

    $listArgs = [System.Collections.Generic.List[string]]::new()
    $listArgs.AddRange([string[]]@('ls-files', '--'))
    $listArgs.AddRange($Pattern)
    $files = & git @listArgs
    if ($LASTEXITCODE -ne 0) {
        throw "git ls-files failed with exit code $LASTEXITCODE"
    }
    return [string[]]@($files)
}

function Get-AwkCommand {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    $onPath = Get-Command awk -CommandType Application -ErrorAction SilentlyContinue
    if ($onPath) { return @($onPath)[0].Source }

    $git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue
    if ($git) {
        $gitRoot = Split-Path -Parent (Split-Path -Parent @($git)[0].Source)
        $bundled = Join-Path $gitRoot 'usr\bin\awk.exe'
        if (Test-Path -LiteralPath $bundled) { return $bundled }
    }

    throw 'awk was not found. Install Git for Windows or add awk to PATH.'
}

function Get-ClangFormatCommand {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    if ($env:CLANG_FORMAT) {
        $configured = Get-Command $env:CLANG_FORMAT -ErrorAction SilentlyContinue
        if ($configured) { return $configured.Source }
        if (Test-Path -LiteralPath $env:CLANG_FORMAT) {
            return (Resolve-Path -LiteralPath $env:CLANG_FORMAT).Path
        }
        throw "CLANG_FORMAT is set but was not found: $env:CLANG_FORMAT"
    }

    $versioned = Get-Command clang-format-19 -ErrorAction SilentlyContinue
    if ($versioned) { return $versioned.Source }

    $programFilesX86 = ${env:ProgramFiles(x86)}
    if ($programFilesX86) {
        $vsRelative = 'Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\bin\clang-format.exe'
        $vsClangFormat = Join-Path $programFilesX86 $vsRelative
        if (Test-Path -LiteralPath $vsClangFormat) {
            return (Resolve-Path -LiteralPath $vsClangFormat).Path
        }
    }

    $pathClangFormat = Get-Command clang-format -ErrorAction SilentlyContinue
    if ($pathClangFormat) { return $pathClangFormat.Source }

    throw 'clang-format was not found. Install LLVM clang-format and rerun .github/lint/lint.ps1.'
}

function Invoke-RepoCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param()

    Write-Host 'Checking code style, spelling, workflows, secrets and dependencies...'
    Invoke-Uv -Arguments @('python', '-B', '.github/lint/style_check.py')
    Invoke-Uv -Arguments @('typos', '--config', '.github/lint/typos.toml', '.')
    Invoke-Uv -Arguments @('zizmor', '--offline', '--quiet', '.')
    $gitleaksArgs = @('git', '--no-banner', '--redact', '--log-opts=--branches --not --remotes', '.')
    Invoke-Native -FilePath 'gitleaks' -Arguments $gitleaksArgs
    $denyArgs = @('deny', '--manifest-path', 'bridge/Cargo.toml', 'check', 'advisories', 'bans', 'sources')
    Invoke-Native -FilePath 'cargo' -Arguments $denyArgs
}

function Invoke-PowerShellCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param()

    Write-Host 'Checking PowerShell scripts...'
    Invoke-Native -FilePath 'pwsh' -Arguments @('-NoProfile', '-File', '.github/lint/Invoke-PSLint.ps1')
}

function Invoke-ScriptTest {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool] $OnWindows)

    $tests = @(
        'Test-UpdateChangelog.ps1',
        'Test-GenerateReleaseNotes.ps1',
        'Test-ResolveReleaseBaseTag.ps1',
        'Test-NightlyPrereleasePlan.ps1',
        'Test-CommitHooks.ps1'
    )
    $shells = [System.Collections.Generic.List[string]]::new()
    if ($OnWindows) { $shells.Add('powershell') }
    if (Get-Command pwsh -ErrorAction SilentlyContinue) { $shells.Add('pwsh') }

    $modulePath = $env:PSModulePath
    try {
        foreach ($shell in $shells) {
            Write-Host "Running the release script tests under $shell..."
            $env:PSModulePath = if ($shell -eq 'powershell') { $null } else { $modulePath }
            foreach ($test in $tests) {
                $testPath = Join-Path '.github/scripts' $test
                Invoke-Native -FilePath $shell -Arguments @(
                    '-NoProfile',
                    '-ExecutionPolicy',
                    'Bypass',
                    '-File',
                    $testPath
                )
            }
        }
    } finally {
        $env:PSModulePath = $modulePath
    }
}

function Invoke-RustCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param([switch] $Fix)

    Write-Host 'Checking Rust formatting...'
    if ($Fix) {
        Invoke-Native -FilePath 'cargo' -Arguments @('fmt', '--manifest-path', 'bridge/Cargo.toml', '--all')
    } else {
        Invoke-Native -FilePath 'cargo' -Arguments @(
            'fmt',
            '--manifest-path',
            'bridge/Cargo.toml',
            '--all',
            '--',
            '--check'
        )
    }

    Write-Host 'Running Clippy...'
    Invoke-Native -FilePath 'cargo' -Arguments @(
        'clippy',
        '--manifest-path',
        'bridge/Cargo.toml',
        '--workspace',
        '--all-targets',
        '--locked',
        '--',
        '-D',
        'warnings'
    )

    Write-Host 'Running Rust tests...'
    Invoke-Native -FilePath 'cargo' -Arguments @(
        'test',
        '--manifest-path',
        'bridge/Cargo.toml',
        '--workspace',
        '--all-targets',
        '--locked'
    )

    $rustFiles = Get-TrackedFile -Pattern @('*.rs')
    if ($rustFiles.Count -gt 0) {
        Write-Host 'Checking Rust style...'
        $awkArgs = [System.Collections.Generic.List[string]]::new()
        $awkArgs.AddRange([string[]]@('-f', '.github/lint/rust-style.awk'))
        $awkArgs.AddRange([string[]]$rustFiles)
        Invoke-Native -FilePath (Get-AwkCommand) -Arguments $awkArgs.ToArray()
    }
}

function Invoke-PythonCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param([switch] $Fix)

    Write-Host 'Checking Python...'
    if ($Fix) {
        Invoke-Uv -Arguments @('ruff', 'format', '.')
    } else {
        Invoke-Uv -Arguments @('ruff', 'format', '--check', '.')
    }
    Invoke-Uv -Arguments @('ruff', 'check', '--ignore-noqa', '.')
    Invoke-Uv -Arguments @('pyright', '--warnings', 'scripts', 'tests', '.github/lint')
    Invoke-Uv -Arguments @('python', '-I', '.github/lint/pystyle.py')
    Invoke-Uv -Arguments @('python', '-B', '-m', 'unittest', 'discover', '-s', 'tests', '-t', '.')
}

function Invoke-ShellCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [bool] $OnWindows,
        [Parameter(Mandatory = $true)][string] $RepoRoot
    )

    Write-Host 'Checking shell scripts and workflows...'
    if (-not $OnWindows) {
        Invoke-Native -FilePath 'bash' -Arguments @('.githooks/check-shell')
        return
    }
    if (-not $env:LINT_WSL_DISTRO) {
        throw 'Set LINT_WSL_DISTRO to the WSL distro that has shellcheck, shfmt and actionlint.'
    }
    Invoke-Native -FilePath 'wsl' -Arguments @(
        '--distribution',
        $env:LINT_WSL_DISTRO,
        '--cd',
        $RepoRoot,
        '--',
        'bash',
        '.githooks/check-shell'
    )
}

function Invoke-FirmwareCheck {
    [CmdletBinding()]
    [OutputType([void])]
    param(
        [switch] $Fix,
        [Parameter(Mandatory = $true)][string] $RepoRoot
    )

    $clangFormat = Get-ClangFormatCommand
    Write-Host "Using $clangFormat"
    Invoke-Native -FilePath $clangFormat -Arguments @('--version')

    $firmwareRoot = Join-Path $RepoRoot 'pico-bridge'
    $firmwareRoots = @(
        (Join-Path $firmwareRoot 'src'),
        (Join-Path $firmwareRoot 'tests')
    )
    $sources = Get-ChildItem -LiteralPath $firmwareRoots -Recurse -File |
        Where-Object { $_.Extension -eq '.c' -or $_.Extension -eq '.h' } |
        Sort-Object FullName

    $formatArgs = [System.Collections.Generic.List[string]]::new()
    if ($Fix) {
        $formatArgs.Add('-i')
    } else {
        $formatArgs.AddRange([string[]]@('--dry-run', '--Werror'))
    }
    $optionCount = $formatArgs.Count
    foreach ($source in $sources) {
        $formatArgs.Add($source.FullName)
    }
    if ($formatArgs.Count -eq $optionCount) { return }

    Write-Host 'Checking firmware C formatting...'
    Invoke-Native -FilePath $clangFormat -Arguments $formatArgs.ToArray()
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
Set-Location -LiteralPath $repoRoot
$onWindows = $env:OS -eq 'Windows_NT'

if ($Section -contains 'Repo') { Invoke-RepoCheck }
if ($Section -contains 'PowerShell') { Invoke-PowerShellCheck }
if ($Section -contains 'ScriptTests') { Invoke-ScriptTest -OnWindows $onWindows }
if ($Section -contains 'Rust') { Invoke-RustCheck -Fix:$Fix }
if ($Section -contains 'Python') { Invoke-PythonCheck -Fix:$Fix }
if ($Section -contains 'Shell') { Invoke-ShellCheck -OnWindows $onWindows -RepoRoot $repoRoot }
if ($Section -contains 'Firmware') { Invoke-FirmwareCheck -Fix:$Fix -RepoRoot $repoRoot }

Write-Host 'Lint checks passed.'
exit 0
