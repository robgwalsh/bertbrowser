<#
.SYNOPSIS
    Installs BertBrowser: the latest release, the unstable build, or this checkout's own build.

.DESCRIPTION
    From anywhere, in Windows PowerShell 5.1 or PowerShell 7:

        irm https://raw.githubusercontent.com/robgwalsh/bertbrowser/main/install.ps1 | iex
        iex "& { $(irm https://raw.githubusercontent.com/robgwalsh/bertbrowser/main/install.ps1) } -Channel unstable"
        iex "& { $(irm https://raw.githubusercontent.com/robgwalsh/bertbrowser/main/install.ps1) } -Version 2.0.1"

    From a checkout, its own build:

        pwsh .\install.ps1 -Local            # out\release\app into the installed copy
        pwsh .\install.ps1 -Local -Build     # build the release preset first
        pwsh .\install.ps1 -Local -Watch     # and again whenever a build changes it

    Every source lands in the same place, Velopack's per-user install in %LOCALAPPDATA%\BertBrowser
    (no admin prompt), so anything looking for an installed BertBrowser finds whichever went in last.
    Data in %USERPROFILE%\.bertbrowser is never touched.

    A local build is its own Velopack channel, "local": the first one is packed with vpk and installed
    by its Setup.exe like any release, and from then on an install copies only the executables that
    changed, which takes about a second. A -local version never updates itself from GitHub.
    Installing a release or unstable over it puts that copy back on its channel. Setup starts the app
    when it finishes; the copying path never does. See docs/build-and-release.md, "Installing from the
    command line".

.PARAMETER Channel
    stable (default) or unstable. Also read from $env:BERTBROWSER_CHANNEL, for the `| iex` form.
.PARAMETER Version
    A specific release (2.0.1 or v2.0.1) instead of the latest.
.PARAMETER Local
    Install this checkout's build (out\<Config>\app) instead of a download.
.PARAMETER Config
    With -Local: which build, release (default) or debug.
.PARAMETER Build
    With -Local: run build.ps1 first.
.PARAMETER Watch
    With -Local: keep running and install again each time a build changes the executables.
.PARAMETER Repack
    With -Local: pack and run Setup even when a local install exists (refreshes shortcuts and the
    Apps & features entry; the fast path only replaces files).
.PARAMETER InstallRoot
    Where to install. Only for testing: everything else expects the default.
#>
param(
    [ValidateSet('stable', 'unstable')] [string] $Channel,
    [string] $Version,
    [switch] $Local,
    [ValidateSet('debug', 'release')] [string] $Config = 'release',
    [switch] $Build,
    [switch] $Watch,
    [switch] $Repack,
    [string] $InstallRoot = (Join-Path $env:LOCALAPPDATA 'BertBrowser')
)

# A child scope, so that run through `| iex` nothing below leaks into the caller's session - and no
# `exit`, which would close it.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Windows PowerShell's progress bar slows downloads tenfold
    $repo = 'https://github.com/robgwalsh/bertbrowser'
    $defaultRoot = Join-Path $env:LOCALAPPDATA 'BertBrowser'
    $current = Join-Path $InstallRoot 'current'
    $root = $PSScriptRoot   # empty when run through iex
    if (-not $Channel) { $Channel = if ($env:BERTBROWSER_CHANNEL) { $env:BERTBROWSER_CHANNEL } else { 'stable' } }

    function Get-InstalledVersion {
        $file = Join-Path $current 'sq.version'
        if (-not (Test-Path $file)) { return $null }
        $m = [regex]::Match((Get-Content $file -Raw), '<version>\s*([^<\s]+)\s*</version>')
        if ($m.Success) { $m.Groups[1].Value } else { $null }
    }

    # Closes the installed copy, not a dev build running from out\. Politely first, so the settings
    # debounce flushes. The elevated index helper cannot be stopped from here and need not be: it exits
    # by itself when the executables beside it change.
    function Stop-InstalledApp {
        $prefix = $current.TrimEnd('\') + '\'
        $running = @(Get-Process BertBrowser -ErrorAction SilentlyContinue |
                Where-Object { $_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        if (-not $running) { return }
        Write-Host 'Closing the installed BertBrowser...'
        foreach ($p in $running) { [void]$p.CloseMainWindow() }
        foreach ($p in $running) { if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force } }
    }

    function Invoke-Setup([string] $setup) {
        Stop-InstalledApp
        $log = Join-Path ([IO.Path]::GetTempPath()) "bertbrowser-setup-$PID.log"
        $arguments = "--silent --log `"$log`""
        if ($InstallRoot -ne $defaultRoot) { $arguments += " --installto `"$InstallRoot`"" }
        # Process.WaitForExit, not Start-Process -Wait: Setup starts the app when it finishes, and
        # -Wait would wait for that too.
        $p = [Diagnostics.Process]::Start($setup, $arguments)
        $p.WaitForExit()
        if ($p.ExitCode -ne 0) { throw "Setup failed with exit code $($p.ExitCode); its log is $log" }
        if (-not (Test-Path (Join-Path $current 'BertBrowser.exe'))) { throw "Setup finished but $current\BertBrowser.exe is missing; its log is $log" }
    }

    function Install-FromGitHub {
        $tag = $Version.Trim().TrimStart('v')
        if ($tag) {
            $url = "$repo/releases/download/v$tag/BertBrowser-win-Setup.exe"; $what = "BertBrowser $tag"
        }
        elseif ($Channel -eq 'unstable') {
            $url = "$repo/releases/download/unstable/BertBrowser-unstable-Setup.exe"; $what = 'the unstable build'
        }
        else {
            $url = "$repo/releases/latest/download/BertBrowser-win-Setup.exe"; $what = 'the latest release'
        }
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $dir = Join-Path ([IO.Path]::GetTempPath()) ('bertbrowser-install-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory $dir | Out-Null
        try {
            $setup = Join-Path $dir 'BertBrowser-Setup.exe'
            Write-Host "Downloading $what..."
            Invoke-WebRequest $url -OutFile $setup -UseBasicParsing
            Write-Host 'Installing...'
            Invoke-Setup $setup
        }
        finally { Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue }
    }

    # The shipped files, staged by the same script CI packs from, so the list of what ships lives in
    # one place.
    function Get-StagedBuild {
        $app = Join-Path $root "out\$Config\app"
        $pack = Join-Path $root "out\local-install\pack"
        & (Join-Path $root 'tools\stage-package.ps1') -Build $app -Out $pack 6>$null
        $pack
    }

    function Get-LocalVersion {
        $tag = git -C $root tag --list 'v[0-9]*' --sort=-v:refname | Select-Object -First 1
        if (-not $tag) { $tag = 'v0.0.0' }
        $major, $minor, $patch = $tag.TrimStart('v').Split('.')
        "$major.$minor.$([int]$patch + 1)-local.$(Get-Date -Format yyyyMMddHHmmss)"
    }

    # Once per machine (or with -Repack): a real Velopack package, so the install has its Update.exe,
    # shortcuts and Apps & features entry exactly as a release's would be.
    function Install-LocalPackage {
        if (-not (Get-Command vpk -ErrorAction SilentlyContinue)) {
            throw 'The first local install packs with vpk: dotnet tool install -g vpk'
        }
        $pack = Get-StagedBuild
        $version = Get-LocalVersion
        $out = Join-Path $root 'out\local-install\releases'
        if (Test-Path $out) { Remove-Item $out -Recurse -Force }
        Write-Host "Packing $version (first local install; later ones copy only what changed)..."
        vpk pack --packId BertBrowser --packVersion $version --packDir $pack --mainExe BertBrowser.exe `
            --packTitle BertBrowser --packAuthors 'Rob Walsh' --icon (Join-Path $root 'src\app\app\app.ico') `
            --channel local --delta None --outputDir $out | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "vpk pack failed with exit code $LASTEXITCODE" }
        Invoke-Setup (Join-Path $out 'BertBrowser-local-Setup.exe')
    }

    # Every later install: replace whichever executables differ. A file still running (the elevated
    # index helper) cannot be overwritten but can be renamed, so it is moved aside and cleared next time.
    function Sync-LocalBuild {
        $pack = Get-StagedBuild
        $changed = @(Get-ChildItem $pack -File | Where-Object {
                $dst = Join-Path $current $_.Name
                -not (Test-Path $dst) -or $_.Length -ne (Get-Item $dst).Length -or
                (Get-FileHash $_.FullName).Hash -ne (Get-FileHash $dst).Hash
            })
        Get-ChildItem $current -Filter '*.old-*' -File | Remove-Item -ErrorAction SilentlyContinue
        if (-not $changed) { Write-Host 'The installed copy already matches the build.'; return }
        Stop-InstalledApp
        foreach ($file in $changed) {
            $dst = Join-Path $current $file.Name
            try { Copy-Item $file.FullName $dst -Force }
            catch {
                Move-Item $dst "$dst.old-$([DateTime]::UtcNow.Ticks)"
                Copy-Item $file.FullName $dst -Force
            }
        }
        Write-Host ("Installed {0} -> {1}" -f (($changed | ForEach-Object Name) -join ', '), $current)
    }

    function Install-Local {
        $installed = Get-InstalledVersion
        if ($Repack -or -not $installed -or $installed -notmatch '-local(\.|$)') { Install-LocalPackage }
        else { Sync-LocalBuild }
    }

    if (-not $Local) {
        if ($Build -or $Watch -or $Repack) { throw '-Build, -Watch and -Repack go with -Local.' }
        Install-FromGitHub
        Write-Host "Installed BertBrowser $(Get-InstalledVersion) to $current"
        return
    }

    if (-not $root -or -not (Test-Path (Join-Path $root 'tools\stage-package.ps1'))) {
        throw '-Local installs a checkout''s build: run .\install.ps1 -Local from the repository.'
    }
    if ($Build) {
        & pwsh -NoProfile -File (Join-Path $root 'build.ps1') -Config $Config
        if ($LASTEXITCODE -ne 0) { throw "build.ps1 failed with exit code $LASTEXITCODE" }
    }
    if (-not $Watch) {
        Install-Local
        return
    }

    # Polls rather than using a FileSystemWatcher: a link writes an exe several times, and what matters
    # is the moment the set stops changing, which a poll sees directly.
    $app = Join-Path $root "out\$Config\app"
    $stamp = { (Get-ChildItem (Join-Path $app '*.exe') | ForEach-Object { "$($_.Name):$($_.LastWriteTimeUtc.Ticks):$($_.Length)" }) -join '|' }
    Write-Host "Watching $app - every build that changes it is installed. Ctrl+C to stop."
    $last = $null
    while ($true) {
        $now = & $stamp
        if ($now -ne $last) {
            Start-Sleep -Seconds 2
            if ((& $stamp) -eq $now) {
                $last = $now
                $started = Get-Date
                try { Install-Local; Write-Host ("  ({0:N1} s, {1:HH:mm:ss})" -f ((Get-Date) - $started).TotalSeconds, (Get-Date)) }
                catch { Write-Warning $_ }
            }
            continue
        }
        Start-Sleep -Seconds 1
    }
}
