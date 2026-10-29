# Builds the Velopack installer locally from the native tree. Prereq: dotnet tool install -g vpk, and the
# native toolchain (see native/README.md). Output lands in Releases\ (Setup.exe, full/delta packages,
# portable zip). Running it again with a higher version against the same Releases\ dir produces a delta.
#
# Does what CI does (.github/actions/native-build): checks, a release build, and the harness scripts, with
# the version stamped in after the tests. Stamping edits native/Cargo.toml, native/CMakeLists.txt and
# native/Cargo.lock; they are put back afterwards so a local pack leaves the tree as it was.
param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$native = Join-Path $root 'native'

function Step([scriptblock] $Block) {
    & $Block
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# Enter the Visual Studio developer shell (cl, link, cmake, ninja on PATH), as native/build.ps1 does.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'No Visual Studio with the C++ toolset found.' }
Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null

# What the version stamp edits, kept to put back exactly as it was (a checkout would also drop local changes).
$stamped = 'Cargo.toml', 'CMakeLists.txt', 'Cargo.lock' | ForEach-Object { Join-Path $native $_ }
$saved = @{}
foreach ($f in $stamped) { $saved[$f] = [System.IO.File]::ReadAllBytes($f) }

Push-Location $native
try {
    Step { cargo fmt --all --check }
    Step { cargo clippy --workspace --all-targets -- -D warnings }
    Step { cargo test --workspace --release }
    Step { pwsh "$native\tools\set-version.ps1" -Version $Version }
    Step { cmake --preset release }
    Step { cmake --build --preset release }
    Step { pwsh "$native\tools\run-harness-scripts.ps1" }
    Step { pwsh "$native\tools\stage-package.ps1" -Out "$native\out\release\pack" }

    # The pack id is BertBrowser, so an installed C# copy takes this build as its next update.
    Step {
        vpk pack --packId BertBrowser --packVersion $Version --packDir "$native\out\release\pack" `
            --mainExe BertBrowser.exe --packTitle BertBrowser --packAuthors 'Rob Walsh' `
            --icon "$native\app\app\app.ico" --outputDir "$root\Releases"
    }
}
finally {
    Pop-Location
    foreach ($f in $stamped) { [System.IO.File]::WriteAllBytes($f, $saved[$f]) }
}
