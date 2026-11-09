#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Tag
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-IsPrereleaseTag {
    [CmdletBinding()]
    [OutputType([bool])]
    param([Parameter(Mandatory = $true)][string] $Tag)

    return $Tag -like '*-*'
}

$describeArgs = [System.Collections.Generic.List[string]]::new()
$describeArgs.AddRange([string[]]@('describe', '--tags', '--abbrev=0'))
if (-not (Test-IsPrereleaseTag -Tag $Tag)) {
    $describeArgs.AddRange([string[]]@('--exclude', '*-*'))
}
$describeArgs.Add("$Tag^")

$prevRef = & { $ErrorActionPreference = 'Continue'; git @describeArgs 2>$null }
if ($LASTEXITCODE -eq 0 -and $prevRef) {
    $prevRef.Trim()
}

exit 0
