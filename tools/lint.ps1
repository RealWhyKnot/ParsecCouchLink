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
    $clangFormat = Get-Command clang-format -ErrorAction SilentlyContinue
    if (-not $clangFormat) {
        throw "clang-format was not found. Install LLVM clang-format and rerun tools/lint.ps1."
    }

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
            Invoke-Native -FilePath $clangFormat.Source -Arguments (@("-i") + $firmwareFiles)
        } else {
            Invoke-Native -FilePath $clangFormat.Source -Arguments (@("--dry-run", "--Werror") + $firmwareFiles)
        }
    }
}

Write-Host "Lint checks passed."
