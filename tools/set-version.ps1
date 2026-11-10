<#
.SYNOPSIS
    Stamps a release version into the build, so the About box and bb_version() report what was
    tagged. CI runs it before building; a local build keeps the committed 2.0.0.

.DESCRIPTION
    bb_version() reports BB_VERSION when the build sets it, else Cargo.toml's [workspace.package]
    version (bb-ffi's lifecycle.rs). The version is passed that way rather than rewritten into
    Cargo.toml because every workspace crate inherits Cargo.toml's version: rewriting it after the
    tests recompiled the whole workspace for the release build, where the variable recompiles bb-ffi
    alone. On CI this script exports BB_VERSION to the steps after it (GITHUB_ENV); by hand, dot-source
    it so the variable lands in your session. The CMake project() call takes the numeric part,
    rewritten here.

.EXAMPLE
    pwsh tools/set-version.ps1 -Version 2.0.1-unstable.42       # CI
    . tools/set-version.ps1 -Version 2.0.1-unstable.42          # by hand, then build
#>
[CmdletBinding()]
param([Parameter(Mandatory)][string] $Version)
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^(\d+\.\d+\.\d+)(-[0-9A-Za-z.-]+)?$') { throw "not a SemVer version: '$Version'" }
$numeric = $Matches[1]
$root = Join-Path $PSScriptRoot '..'
$utf8 = New-Object System.Text.UTF8Encoding $false

function Rewrite([string] $path, [string] $pattern, [string] $replacement) {
    $text = [System.IO.File]::ReadAllText($path)
    $re = [regex]::new($pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)
    if (-not $re.IsMatch($text)) { throw "${path}: no match for ${pattern}" }
    [System.IO.File]::WriteAllText($path, $re.Replace($text, $replacement, 1), $utf8)
}

Rewrite (Join-Path $root 'CMakeLists.txt') '(project\(bertbrowser VERSION )[0-9.]+' "`${1}$numeric"
$env:BB_VERSION = $Version
if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value "BB_VERSION=$Version" -Encoding utf8 }
Write-Host "version is now $Version (BB_VERSION)"
