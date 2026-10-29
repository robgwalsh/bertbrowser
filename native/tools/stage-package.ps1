<#
.SYNOPSIS
    Copies what ships (BertBrowser.exe and the two elevated helpers beside it) out of a release build
    into a clean folder for `vpk pack --packDir`.

.DESCRIPTION
    native/out/release/app also holds the build's own files (static libraries, CMake scripts), and
    vpk packs whatever is in its folder, so the package is staged from an explicit list rather than
    filtered out of the build tree. No .pdb: the C# build did not ship them either.

.EXAMPLE
    pwsh native/tools/stage-package.ps1 -Out native/out/release/pack
#>
[CmdletBinding()]
param(
    [string] $Build = (Join-Path $PSScriptRoot '..\out\release\app'),
    [string] $Out = (Join-Path $PSScriptRoot '..\out\release\pack')
)
$ErrorActionPreference = 'Stop'
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory -Force $Out | Out-Null
foreach ($name in 'BertBrowser.exe', 'bb-indexer.exe', 'bb-elevator.exe') {
    $src = Join-Path $Build $name
    if (-not (Test-Path $src)) { throw "$src is missing: build the release preset first" }
    Copy-Item $src $Out
}
Get-ChildItem $Out | ForEach-Object { '{0,-20} {1,10:N0} bytes' -f $_.Name, $_.Length | Write-Host }
