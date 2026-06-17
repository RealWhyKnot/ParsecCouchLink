#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch]$Fix,
    [switch]$SkipRust,
    [switch]$SkipFirmware,
    [switch]$SkipPowerShell
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $RepoRoot

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Get-ClangFormatCommand {
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
        $vsClangFormat = Join-Path $programFilesX86 "Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\bin\clang-format.exe"
        if (Test-Path -LiteralPath $vsClangFormat) {
            return (Resolve-Path -LiteralPath $vsClangFormat).Path
        }
    }

    $pathClangFormat = Get-Command clang-format -ErrorAction SilentlyContinue
    if ($pathClangFormat) { return $pathClangFormat.Source }

    throw "clang-format was not found. Install LLVM clang-format and rerun tools/lint.ps1."
}

if (-not $SkipPowerShell) {
    Write-Host "Checking PowerShell syntax and changelog updater..."
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-WorkflowSyntax.ps1"
    )
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-UpdateChangelog.ps1"
    )
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-GenerateReleaseNotes.ps1"
    )
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-ResolveReleaseBaseTag.ps1"
    )
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-ReleaseVersionSequence.ps1"
    )
    Invoke-Native -FilePath "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ".\.github\scripts\Test-CommitHooks.ps1"
    )
}

if (-not $SkipRust) {
    Write-Host "Checking Rust formatting..."
    if ($Fix) {
        Invoke-Native -FilePath "cargo" -Arguments @("fmt", "--manifest-path", "bridge\Cargo.toml")
    } else {
        Invoke-Native -FilePath "cargo" -Arguments @(
            "fmt",
            "--manifest-path",
            "bridge\Cargo.toml",
            "--",
            "--check"
        )
    }

    Write-Host "Running Clippy..."
    Invoke-Native -FilePath "cargo" -Arguments @(
        "clippy",
        "--manifest-path",
        "bridge\Cargo.toml",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings"
    )
}

if (-not $SkipFirmware) {
    $clangFormat = Get-ClangFormatCommand
    Write-Host "Using $clangFormat"
    Invoke-Native -FilePath $clangFormat -Arguments @("--version")

    $FirmwareRoot = Join-Path $RepoRoot "pico-bridge"
    $firmwareRoots = @(
        (Join-Path $FirmwareRoot "src"),
        (Join-Path $FirmwareRoot "tests")
    )
    $firmwareFiles = Get-ChildItem -LiteralPath $firmwareRoots -Recurse -File |
        Where-Object { $_.Extension -eq ".c" -or $_.Extension -eq ".h" } |
        Sort-Object FullName |
        ForEach-Object { $_.FullName }

    if ($firmwareFiles.Count -gt 0) {
        Write-Host "Checking firmware C formatting..."
        if ($Fix) {
            Invoke-Native -FilePath $clangFormat -Arguments (@("-i") + $firmwareFiles)
        } else {
            Invoke-Native -FilePath $clangFormat -Arguments (@("--dry-run", "--Werror") + $firmwareFiles)
        }
    }
}

Write-Host "Lint checks passed."
