# Builds the Velopack installer locally from the repo. Prereq: dotnet tool install -g vpk, and the
# C++ and Rust toolchains (see the README build section). Output lands in Releases\ (Setup.exe, full/delta packages,
# portable zip). Running it again with a higher version against the same Releases\ dir produces a delta.
#
# Does what CI does (.github/actions/build): checks, a release build, and the harness scripts, with
# the version stamped in after the tests. Stamping edits Cargo.toml, CMakeLists.txt and
# Cargo.lock; they are put back afterwards so a local pack leaves the tree as it was.
param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

function Step([scriptblock] $Block) {
    & $Block
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# Enter the Visual Studio developer shell (cl, link, cmake, ninja on PATH), as build.ps1 does.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'No Visual Studio with the C++ toolset found.' }
Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null

# What the version stamp edits, kept to put back exactly as it was (a checkout would also drop local changes).
$stamped = 'Cargo.toml', 'CMakeLists.txt', 'Cargo.lock' | ForEach-Object { Join-Path $root $_ }
$saved = @{}
foreach ($f in $stamped) { $saved[$f] = [System.IO.File]::ReadAllBytes($f) }

Push-Location $root
try {
    Step { cargo fmt --all --check }
    Step { cargo clippy --workspace --all-targets -- -D warnings }
    Step { cargo test --workspace --release }
    Step { pwsh "$root\tools\set-version.ps1" -Version $Version }
    Step { cmake --preset release }
    Step { cmake --build --preset release }
    Step { pwsh "$root\tools\run-harness-scripts.ps1" }
    Step { pwsh "$root\tools\stage-package.ps1" -Out "$root\out\release\pack" }

    # The pack id is BertBrowser, so an installed C# copy takes this build as its next update.
    Step {
        vpk pack --packId BertBrowser --packVersion $Version --packDir "$root\out\release\pack" `
            --mainExe BertBrowser.exe --packTitle BertBrowser --packAuthors 'Rob Walsh' `
            --icon "$root\app\app\app.ico" --outputDir "$root\Releases"
    }
}
finally {
    Pop-Location
    foreach ($f in $stamped) { [System.IO.File]::WriteAllBytes($f, $saved[$f]) }
}
