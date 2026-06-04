#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$Root = (Get-Location).Path
)

$ErrorActionPreference = "Stop"

$errors = [System.Collections.Generic.List[string]]::new()

function Add-ParserErrors {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)]$ParseErrors
    )

    foreach ($parseError in $ParseErrors) {
        $line = $parseError.Extent.StartLineNumber
        $col = $parseError.Extent.StartColumnNumber
        $errors.Add("$Source (line ${line}:${col}): $($parseError.Message)") | Out-Null
    }
}

Get-ChildItem -LiteralPath $Root -Filter "*.ps1" -Recurse |
    Where-Object {
        $_.FullName -notmatch "\\bridge\\target\\" -and
        $_.FullName -notmatch "\\dist\\" -and
        $_.FullName -notmatch "\\pico-bridge\\build"
    } |
    ForEach-Object {
        $parseErrors = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile(
            $_.FullName,
            [ref]$null,
            [ref]$parseErrors
        )
        if ($parseErrors) {
            Add-ParserErrors -Source $_.FullName -ParseErrors $parseErrors
        }
    }

$workflowDir = Join-Path $Root ".github\workflows"
if (Test-Path -LiteralPath $workflowDir) {
    $ghaPattern = [regex]"\$\{\{[^}]*\}\}"
    Get-ChildItem -LiteralPath $workflowDir -Filter "*.yml" | ForEach-Object {
        $path = $_.FullName
        $lines = Get-Content -LiteralPath $path
        $stepName = "<unnamed>"
        $isPwsh = $false
        $inRun = $false
        $runIndent = -1
        $runStart = 0
        $block = [System.Collections.Generic.List[string]]::new()

        $flush = {
            if ($block.Count -eq 0) { return }
            $baseline = -1
            foreach ($line in $block) {
                if ($line.Trim().Length -eq 0) { continue }
                $baseline = $line.Length - $line.TrimStart(" ").Length
                break
            }
            if ($baseline -lt 0) { return }

            $body = ($block | ForEach-Object {
                if ($_.Length -gt $baseline) { $_.Substring($baseline) } else { "" }
            }) -join "`n"
            $body = $ghaPattern.Replace($body, "__GHA_EXPR__")

            $parseErrors = $null
            [void][System.Management.Automation.Language.Parser]::ParseInput(
                $body,
                [ref]$null,
                [ref]$parseErrors
            )
            if ($parseErrors) {
                Add-ParserErrors -Source "$path step '$stepName' (run: line $runStart)" -ParseErrors $parseErrors
            }
        }

        for ($i = 0; $i -lt $lines.Count; $i++) {
            $line = $lines[$i]
            $indent = $line.Length - $line.TrimStart(" ").Length
            $trimmed = $line.Trim()

            if ($inRun) {
                if ($trimmed.Length -gt 0 -and $indent -le $runIndent) {
                    & $flush
                    $block.Clear()
                    $inRun = $false
                } else {
                    $block.Add($line) | Out-Null
                    continue
                }
            }

            if ($trimmed -match "^- name:\s*(.+?)\s*$") {
                if ($block.Count -gt 0) {
                    & $flush
                    $block.Clear()
                    $inRun = $false
                }
                $stepName = $matches[1]
                $isPwsh = $false
                continue
            }

            if ($trimmed -match "^shell:\s*(.+?)\s*$") {
                $isPwsh = ($matches[1] -eq "pwsh")
                continue
            }

            if ($isPwsh -and $trimmed -match "^run:\s*\|\s*$") {
                $inRun = $true
                $runIndent = $indent
                $runStart = $i + 1
                $block.Clear()
                continue
            }
        }
        if ($inRun) { & $flush }
    }
}

if ($errors.Count -gt 0) {
    Write-Host "PowerShell syntax errors:"
    foreach ($message in $errors) {
        Write-Host "  $message"
    }
    throw "Found $($errors.Count) PowerShell syntax error(s)."
}

Write-Host "PowerShell scripts and inline workflow blocks parsed cleanly."
