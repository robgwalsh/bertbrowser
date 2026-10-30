<#
.SYNOPSIS
    Runs every non-bench tools/ui/*.bbs through the offscreen harness, each with its own fresh --out
    directory. Local only (CI skips it: too slow, and one stalled a run); run it before a release.

.DESCRIPTION
    The harness is bb-harness.exe, built with the app (out/<config>/harness). A script stops at
    its first failed command, so a nonzero exit is a real failure; the run goes on to the next script and
    lists everything that failed at the end. Captures land in <Out>\<script name>\ (CI uploads them when
    anything failed).

    Two scripts need flags of their own, as their headers say:
      tree.bbs            --sandbox <visible dir named bertbrowser-treecheck> --allow-outside
      index-degraded.bbs  --index-declined
    bench-*.bbs are measurements, run by `bb-bench ui`, not checks.

.EXAMPLE
    pwsh tools/run-harness-scripts.ps1
    pwsh tools/run-harness-scripts.ps1 -Only smoke,settings,tiles
#>
[CmdletBinding()]
param(
    [string] $Harness = (Join-Path $PSScriptRoot '..\out\release\harness\bb-harness.exe'),
    [string] $Out = (Join-Path $PSScriptRoot '..\out\harness-run'),
    [string[]] $Only = @()
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Harness = (Resolve-Path $Harness).Path
$Out = [System.IO.Path]::GetFullPath($Out)
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })   # `pwsh -File` passes one string
$treeSandbox =Join-Path (Split-Path $repo -Parent) 'bertbrowser-treecheck'

$scripts = Get-ChildItem (Join-Path $repo 'tools\ui') -Filter *.bbs |
    Where-Object { $_.Name -notlike 'bench-*' } |
    Where-Object { -not $Only -or $Only -contains $_.BaseName } |
    Sort-Object Name
if (-not $scripts) { throw 'no scripts selected' }

if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
$failed = @()
foreach ($s in $scripts) {
    $dir = Join-Path $Out $s.BaseName
    New-Item -ItemType Directory -Force $dir | Out-Null
    $harnessArgs = @('--script', $s.FullName, '--out', $dir)
    switch ($s.Name) {
        'tree.bbs' {
            New-Item -ItemType Directory -Force $treeSandbox | Out-Null
            $harnessArgs += @('--sandbox', $treeSandbox, '--allow-outside')
        }
        'index-degraded.bbs' { $harnessArgs += '--index-declined' }
    }
    $log = Join-Path $dir 'harness.log'
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    & $Harness @harnessArgs *> $log
    $code = $LASTEXITCODE
    $status = if ($code -eq 0) { 'ok' } else { 'FAILED' }
    '{0,-24} {1,-7} {2,6:N1}s' -f $s.BaseName, $status, $watch.Elapsed.TotalSeconds | Write-Host
    if ($code -ne 0) {
        $failed += $s.BaseName
        Get-Content $log -Tail 15 | ForEach-Object { Write-Host "    $_" }
    }
}
if (Test-Path $treeSandbox) { Remove-Item $treeSandbox -Recurse -Force -ErrorAction SilentlyContinue }
Write-Host ("{0} script(s), {1} failed" -f $scripts.Count, $failed.Count)
if ($failed) { Write-Host ('failed: ' + ($failed -join ', ')); exit 1 }
