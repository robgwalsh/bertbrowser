#Requires -Version 7
<#
.SYNOPSIS
    Times the three rebuilds that matter day to day, from a warm tree: a bb-core edit through
    build.ps1, a C++ view edit through build.ps1, and a bb-core edit through `cargo test --no-run`.

.DESCRIPTION
    The bb-core scenarios make a real edit (a probe item at the end of format.rs, removed when the
    script ends); the C++ one touches a view. Each times the rebuild that follows, in the order of an
    edit-build-test loop, so switching between build.ps1 and a plain cargo command is part of what is
    measured. For the two build.ps1 scenarios it also reads the Ninja edges that run
    took from out\<config>\.ninja_log, so the cargo runs and the C++ compiles can be told apart.
    Run it once to warm the tree first (the first run after a clean is not a measurement), then
    record the second run. -Label names the run in the output.

.EXAMPLE
    pwsh tools/build-timing.ps1 -Label baseline
#>
[CmdletBinding()]
param(
    [string] $Label = "run",
    [ValidateSet("debug", "release")] [string] $Config = "debug",
    [ValidateSet("core", "cpp", "test")] [string[]] $Scenario = @("core", "cpp", "test"),
    # The checkout to measure; a copy of this script run from elsewhere can time another revision.
    [string] $Root = (Split-Path $PSScriptRoot)
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path $Root).Path
$ninjaLog = Join-Path $root "out\$Config\.ninja_log"
$coreFile = Join-Path $root "src\crates\bb-core\src\format.rs"
$cppFile = Join-Path $root "src\app\views\dialogs\dialogs.cpp"

function Touch([string] $path) { (Get-Item $path).LastWriteTime = Get-Date }

# A real edit, not a touch: rustc's incremental cache makes a touched file nearly free, which no
# actual change is. A new value each time in one probe item at the end of the file, so dependents
# see bb-core's metadata change as they would after any edit. Removed again when the script ends.
$probeTag = "// build-timing probe"
$coreOriginal = [IO.File]::ReadAllText($coreFile)
function EditCore {
    $probe = "#[allow(dead_code)] const BUILD_TIMING_PROBE: u64 = $([DateTime]::UtcNow.Ticks); $probeTag"
    [IO.File]::WriteAllText($coreFile, $coreOriginal.TrimEnd() + "`n$probe`n")
}

# The Ninja edges a build appended, longest first, with outputs that share one edge folded together.
function NinjaEdges([int] $skip) {
    if (-not (Test-Path $ninjaLog)) { return @() }
    Get-Content $ninjaLog | Select-Object -Skip $skip | Where-Object { $_ -notmatch '^#' } | ForEach-Object {
        $p = $_ -split "`t"
        [pscustomobject]@{ start = [int]$p[0]; end = [int]$p[1]; out = $p[3] }
    } | Group-Object start, end | ForEach-Object {
        $first = $_.Group[0]
        $name = ($_.Group.out | Where-Object { $_ -match '^CMakeFiles/' } | Select-Object -First 1) ?? $first.out
        [pscustomobject]@{ startS = [math]::Round($first.start / 1000, 1); endS = [math]::Round($first.end / 1000, 1); edge = $name }
    } | Sort-Object startS
}

function TimeBuild([string] $name, [scriptblock] $change) {
    & $change
    $before = (Test-Path $ninjaLog) ? (Get-Content $ninjaLog).Count : 0
    $t = Measure-Command { pwsh -NoProfile -File (Join-Path $root "build.ps1") -Config $Config | Out-Host }
    if ($LASTEXITCODE) { throw "$name failed" }
    $edges = @(NinjaEdges $before)
    $cargo = $edges | Where-Object { $_.edge -match 'cargo_build' }
    $objs = @($edges | Where-Object { $_.edge -match '\.obj$' })
    [pscustomobject]@{
        label = $Label; scenario = $name; seconds = [math]::Round($t.TotalSeconds, 1)
        detail = (@($cargo | ForEach-Object { "$($_.edge -replace 'CMakeFiles/','') $($_.startS)-$($_.endS)s" }) +
                  ($objs.Count ? @("$($objs.Count) obj $(($objs.startS | Measure-Object -Minimum).Minimum)-$(($objs.endS | Measure-Object -Maximum).Maximum)s") : @())) -join "; "
    }
}

try {
    $results = foreach ($s in $Scenario) {
        switch ($s) {
            "core" { TimeBuild "bb-core edit -> build.ps1" { EditCore } }
            "cpp" { TimeBuild "views .cpp touch -> build.ps1" { Touch $cppFile } }
            "test" {
                EditCore
                Push-Location $root
                try { $t = Measure-Command { cargo test --workspace --no-run 2>&1 | Out-Host } } finally { Pop-Location }
                if ($LASTEXITCODE) { throw "cargo test --no-run failed" }
                [pscustomobject]@{ label = $Label; scenario = "bb-core edit -> cargo test --no-run"; seconds = [math]::Round($t.TotalSeconds, 1); detail = "" }
            }
        }
    }
    $results | Format-Table -AutoSize -Wrap | Out-String -Width 220
}
finally {
    if ([IO.File]::ReadAllText($coreFile) -ne $coreOriginal) { [IO.File]::WriteAllText($coreFile, $coreOriginal) }
}
