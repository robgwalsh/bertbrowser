# Builds the Velopack installer locally. Prereq: dotnet tool install -g vpk
# Output lands in Releases\ (Setup.exe, full/delta packages, portable zip).
# Running it again with a higher version against the same Releases\ dir produces a delta.
param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

dotnet test "$root\bertbrowser.sln" -c Release
if ($LASTEXITCODE -ne 0) { exit 1 }

dotnet publish "$root\src\BertBrowser.App\BertBrowser.App.csproj" -c Release `
    -r win-x64 --self-contained true -p:Version=$Version -o "$root\publish"
if ($LASTEXITCODE -ne 0) { exit 1 }

vpk pack --packId BertBrowser --packVersion $Version --packDir "$root\publish" `
    --mainExe BertBrowser.exe --packTitle BertBrowser --packAuthors "Rob Walsh" `
    --icon "$root\src\BertBrowser.App\Assets\app.ico" --outputDir "$root\Releases"
exit $LASTEXITCODE
