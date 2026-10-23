#Requires -Version 7
# Compares the C# test suite with the Rust one, file by file and name by name.
#
# A C# test file Foo/BarTests.cs maps to a Rust file bar_tests.rs anywhere under native/crates; a C#
# method Some_TestName maps to a Rust fn some_test_name. A [Theory] counts once (the Rust port loops
# over its rows). Deliberately dropped tests are listed in dropped.txt as "BarTests.cs::MethodName".
#
#   .\tests.ps1            files that have a Rust counterpart; exit 1 if any test is unaccounted for
#   .\tests.ps1 -All       every C# file, including those not ported yet (never fails)
#   .\tests.ps1 -File PathKeyTests.cs
param(
    [switch] $All,
    [string] $File,
    [string] $Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
)
$ErrorActionPreference = "Stop"

$csDir = Join-Path $Root "tests\BertBrowser.Core.Tests"
$rsDir = Join-Path $Root "native\crates"
$droppedFile = Join-Path $PSScriptRoot "dropped.txt"

function ConvertTo-Snake([string] $name) {
    $s = [regex]::Replace($name, "([a-z0-9])([A-Z])", '$1_$2')
    $s = [regex]::Replace($s, "([A-Z])([A-Z][a-z])", '$1_$2')
    $s = [regex]::Replace($s, "_+", "_")
    return $s.ToLowerInvariant().Trim("_")
}

$csPattern = [regex]::new('\[(Fact|Theory)[^\]]*\]\s*(?:\[[^\]]*\]\s*)*public\s+(?:async\s+)?(?:static\s+)?[\w<>\[\]]+\s+(\w+)\s*\(', "Singleline")
$rsPattern = [regex]::new('#\[test\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)', "Singleline")

$dropped = @{}
if (Test-Path $droppedFile) {
    foreach ($line in Get-Content $droppedFile) {
        $t = $line.Trim()
        if ($t -eq "" -or $t.StartsWith("#")) { continue }
        $key = ($t -split "\s+")[0]
        $dropped[$key] = $true
    }
}

$rustFiles = @{}
foreach ($f in Get-ChildItem $rsDir -Recurse -Filter *.rs) {
    if ($f.FullName -match "\\target\\") { continue }
    if (-not $rustFiles.ContainsKey($f.Name)) { $rustFiles[$f.Name] = @() }
    $rustFiles[$f.Name] += $f.FullName
}

$csFiles = Get-ChildItem $csDir -Recurse -Filter *Tests.cs | Where-Object { $_.FullName -notmatch "\\(bin|obj)\\" }
if ($File) { $csFiles = $csFiles | Where-Object { $_.Name -eq $File } }

$totalCs = 0; $totalPorted = 0; $totalDropped = 0; $totalMissing = 0; $filesChecked = 0; $filesUnported = 0
foreach ($cs in ($csFiles | Sort-Object Name)) {
    $text = Get-Content $cs.FullName -Raw
    $names = @($csPattern.Matches($text) | ForEach-Object { $_.Groups[2].Value })
    $totalCs += $names.Count
    $rsName = (ConvertTo-Snake ($cs.BaseName)) + ".rs"
    $rsText = ""
    if ($rustFiles.ContainsKey($rsName)) {
        foreach ($p in $rustFiles[$rsName]) { $rsText += (Get-Content $p -Raw) + "`n" }
    }
    elseif (-not $All) {
        $filesUnported++
        continue
    }
    $filesChecked++
    $rsNames = @{}
    foreach ($m in $rsPattern.Matches($rsText)) { $rsNames[$m.Groups[1].Value] = $true }
    $missing = @()
    $droppedHere = 0
    foreach ($n in $names) {
        $snake = ConvertTo-Snake $n
        if ($rsNames.ContainsKey($snake)) { $totalPorted++ }
        elseif ($dropped.ContainsKey("$($cs.Name)::$n")) { $droppedHere++; $totalDropped++ }
        else { $missing += "$n -> $snake" }
    }
    $totalMissing += $missing.Count
    $status = if ($rsText -eq "") { "not ported" } elseif ($missing.Count -eq 0) { "ok" } else { "MISSING $($missing.Count)" }
    "{0,-48} {1,4} tests  {2}" -f $cs.Name, $names.Count, $status
    foreach ($m in $missing) { "    $m" }
}
""
"C# tests: $totalCs   ported: $totalPorted   dropped: $totalDropped   missing: $totalMissing   files checked: $filesChecked   files not ported: $filesUnported"
if (-not $All -and $totalMissing -gt 0) { exit 1 }
exit 0
