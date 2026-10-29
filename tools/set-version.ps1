<#
.SYNOPSIS
    Stamps a release version into the tree, so the About box and bb_version() report what was
    tagged. CI runs it before building; a local build keeps the committed 2.0.0.

.DESCRIPTION
    The version has one source in the Rust workspace (Cargo.toml [workspace.package], which every
    crate inherits and bb_version() returns) and a copy in the CMake project() call. Both are rewritten.
    A SemVer pre-release (2.0.1-unstable.42) is fine for Cargo; CMake takes only the numeric part.
    Cargo.lock follows on the next build, which is why CI does not pass --locked after this.

.EXAMPLE
    pwsh tools/set-version.ps1 -Version 2.0.1-unstable.42
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

# The first `version = "..."` is [workspace.package]'s (the dependency tables come after it).
Rewrite (Join-Path $root 'Cargo.toml') '^version = "[^"]+"' "version = `"$Version`""
Rewrite (Join-Path $root 'CMakeLists.txt') '(project\(bertbrowser VERSION )[0-9.]+' "`${1}$numeric"
Write-Host "version is now $Version"
