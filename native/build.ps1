#Requires -Version 7
# Builds the native tree from any shell: enters the Visual Studio developer environment (cl, link,
# cmake, ninja on PATH), then configures and builds the CMake preset.
#   .\build.ps1                 debug build
#   .\build.ps1 -Config release
#   .\build.ps1 -Target s7_ffi  one target
param(
    [ValidateSet("debug", "release")] [string] $Config = "debug",
    [switch] $Configure,
    [string] $Target
)
$ErrorActionPreference = "Stop"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "No Visual Studio with the C++ toolset found (install the Desktop development with C++ workload)." }
Import-Module (Join-Path $vs "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64" | Out-Null
Push-Location $PSScriptRoot
try {
    if ($Configure -or -not (Test-Path "out\$Config\CMakeCache.txt")) {
        cmake --preset $Config
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
    }
    $buildArgs = @("--build", "--preset", $Config)
    if ($Target) { $buildArgs += @("--target", $Target) }
    cmake @buildArgs
    exit $LASTEXITCODE
}
finally { Pop-Location }
