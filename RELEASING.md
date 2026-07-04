# Releasing BertBrowser

## Ship a new version

```powershell
git tag v1.2.3
git push origin v1.2.3
```

That's it. The `Release` workflow builds, tests, publishes a self-contained win-x64
build, packs it with Velopack (delta against the previous release), and publishes a
GitHub Release with `BertBrowser-win-Setup.exe`, a portable zip, and the update
packages. Installed apps pick the new version up automatically on next launch and
apply it on exit (or immediately if the user accepts the restart prompt).

The version comes from the tag — nothing to bump in any project file.

## Local installer build

```powershell
dotnet tool install -g vpk          # once
scripts\pack.ps1 -Version 1.2.3     # output in Releases\
```

To test the auto-update flow against a local build, set
`BERTBROWSER_UPDATE_URL=C:\Source\bertbrowser\Releases` before launching the
*installed* app. Dev builds (`dotnet run`) never self-update.

## winget

First-time submission (once, after a release is published):

```powershell
winget install wingetcreate         # once
wingetcreate new https://github.com/robgwalsh/bertbrowser/releases/download/v1.0.0/BertBrowser-win-Setup.exe
```

Prompt answers:

| Field | Value |
|---|---|
| PackageIdentifier | `RobWalsh.BertBrowser` |
| InstallerType | `exe` |
| Silent switch | `--silent` |
| Scope | `user` |
| AppsAndFeaturesEntries DisplayName / Publisher | `BertBrowser` / `Rob Walsh` (must match the ARP entry Velopack writes) |

`wingetcreate` submits the PR to `microsoft/winget-pkgs` (one-time Microsoft CLA;
first-package moderation takes a few days).

After that PR merges, automate future versions by adding this job to
`.github/workflows/release.yml`. Two prerequisites:

1. A fork of `microsoft/winget-pkgs` named exactly `winget-pkgs` under your
   account — winget-releaser pushes manifest branches to it. The first
   `wingetcreate` submission creates this fork for you; otherwise:
   `gh repo fork microsoft/winget-pkgs --clone=false`
2. A **classic** PAT with only the `public_repo` scope (fine-grained PATs
   don't work — they can't open PRs against repos you don't own). Create at
   <https://github.com/settings/tokens/new?scopes=public_repo&description=winget-releaser>,
   then store it: `gh secret set WINGET_PAT --repo robgwalsh/bertbrowser`

```yaml
  winget:
    needs: release
    runs-on: windows-latest
    steps:
      - uses: vedantmgoyal9/winget-releaser@v2   # check for the current release tag
        with:
          identifier: RobWalsh.BertBrowser
          installers-regex: 'Setup\.exe$'
          token: ${{ secrets.WINGET_PAT }}
```

Because installed apps update themselves via Velopack, winget lag only affects
first-time installs.
