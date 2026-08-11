#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Tag
)

$ErrorActionPreference = 'Stop'

function Test-IsPrereleaseTag([string]$Tag) {
    return $Tag -like '*-*'
}

$describeArgs = @('describe', '--tags', '--abbrev=0')
if (-not (Test-IsPrereleaseTag -Tag $Tag)) {
    $describeArgs += @('--exclude', '*-*')
}
$describeArgs += "$Tag^"

$prevRef = $null
try {
    $prevRef = & git @describeArgs 2>$null
} catch {
    # Windows PowerShell 5.1 escalates redirected native stderr to a
    # terminating error under $ErrorActionPreference = 'Stop'. pwsh does
    # not; treat both the same as "git describe found nothing".
    $prevRef = $null
}
if ($LASTEXITCODE -eq 0 -and $prevRef) {
    $prevRef.Trim()
}

# Finding no previous tag (first release in a repo) is a valid result,
# reported as empty output. Exit 0 explicitly so git describe's failure
# code does not linger in the caller's $LASTEXITCODE -- GitHub's pwsh
# step wrapper ends every step with `exit $LASTEXITCODE`, and a stale
# 128 here failed the release job with no error output.
exit 0
