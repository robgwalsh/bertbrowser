# Build and release

How BertBrowser is built, packaged, and shipped. The short version: **push a tag, CI does the
rest.** Everything below is the detail behind that.

## Prerequisites

| For | Install |
|---|---|
| Building and testing | [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0) |
| Building an installer locally | `dotnet tool install -g vpk` (Velopack CLI) |
| Publishing a release | Nothing — GitHub Actions does it |
| Updating the winget package by hand | `winget install wingetcreate` |

## Build and test

```powershell
dotnet build bertbrowser.sln       # build everything
dotnet test bertbrowser.sln        # xUnit; BertBrowser.Core.Tests only
dotnet run --project src/BertBrowser.App                  # launch
dotnet run --project src/BertBrowser.App -- C:\Some\Dir   # launch at a folder
```

Two things about this build worth knowing:

- **`Directory.Build.props` sets `TreatWarningsAsErrors`** for every project, so a clean build is a
  warning-free build. A warning that CI hits is a failed release, not a nag.
- **The app manifest requests `requireAdministrator`.** Reading the NTFS MFT needs it, so launching
  triggers UAC — including from `dotnet run` and from the IDE. Only the *app* needs elevation;
  building, testing and packaging do not.

If a build fails with MSB3021/MSB3026 because a running instance is holding `bin\Debug`, kill it and
rebuild:

```powershell
Get-Process BertBrowser | Stop-Process -Force
```

## What a release actually is

`dotnet publish -r win-x64 --self-contained` produces a build with the .NET runtime inside it, and
[Velopack](https://velopack.io) (`vpk`) turns that directory into the release artifacts:

| Asset | What it is |
|---|---|
| `BertBrowser-win-Setup.exe` | The installer. Per-user, into `%LOCALAPPDATA%\BertBrowser`, no admin prompt to install. |
| `BertBrowser-win-Portable.zip` | The same build, unzip and run, nothing installed. |
| `BertBrowser-<version>-full.nupkg` | Full update package, used by installed copies with no matching delta. |
| `BertBrowser-<version>-delta.nupkg` | Only what changed since the previous release — the usual update path, a few MB instead of ~75. |
| `releases.win.json`, `RELEASES` | The update feed installed copies read. |

**The version comes from the tag and nothing else.** No project file carries a version; CI strips the
leading `v` and passes it as `-p:Version=` to `dotnet publish` and `--packVersion` to `vpk`. There is
nothing to bump before tagging.

Versioning is semver as users would read it: a feature release bumps the minor, a fix-only release
bumps the patch.

## Ship a new version

```powershell
git tag v1.2.3
git push origin v1.2.3
```

Tag a commit that's already pushed to `main` — the tag is what the workflow builds, so anything not
on the branch simply isn't in the release.

**Run `dotnet test` before you tag.** The workflow runs the suite and a single red test fails the
release *after* the tag exists, which is the annoying case (see [Recovering a failed
release](#recovering-a-failed-release)).

That push triggers `.github/workflows/release.yml`, which on `windows-latest`:

1. Derives the version from the tag (`v1.2.3` → `1.2.3`).
2. `dotnet test -c Release` — a failure stops everything here.
3. `dotnet publish -c Release -r win-x64 --self-contained true -p:Version=…`.
4. `vpk download github` — pulls the **previous** release so the next step can build a delta against
   it. This fails harmlessly on the very first release; the script logs it and carries on with a full
   package only.
5. `vpk pack` — builds the installer, portable zip, and packages.
6. `vpk upload github --publish` — creates the GitHub Release, tagged with the tag you pushed and
   named `BertBrowser <version>`, and uploads every asset.

Watch it and confirm:

```powershell
gh run watch --exit-status                 # or: gh run list --workflow=release.yml
gh release view v1.2.3 --json assets -q '.assets[].name'
```

A healthy release has six assets, and the delta package should be small. A delta the same size as
the full package means step 4 didn't find the previous release.

### Release notes

`vpk upload` publishes with an empty body, so notes are added afterwards:

```powershell
gh release edit v1.2.3 --notes-file notes.md
```

Write them from what changed for a *user*, grouped by feature area rather than by commit — the commit
subjects in this repo are one-liners and don't carry the story. `git log --oneline v1.1.0..v1.2.3`
and the compare link (`https://github.com/robgwalsh/bertbrowser/compare/v1.1.0...v1.2.3`) are the
starting point; [v1.1.0](https://github.com/robgwalsh/bertbrowser/releases/tag/v1.1.0) is the shape
to copy.

### Recovering a failed release

The tag is the trigger, so a failed run is fixed by fixing the problem and re-tagging the same
version — as long as nothing was published yet:

```powershell
git tag -d v1.2.3
git push origin :refs/tags/v1.2.3     # delete the remote tag
# fix, commit, push to main, then tag again
```

If a run failed *after* the release was created, delete the release too
(`gh release delete v1.2.3 --cleanup-tag`) rather than leaving a half-uploaded one for installed apps
to find. For a run that failed on something transient, `gh run rerun <id>` is enough.

## Build an installer locally

```powershell
scripts\pack.ps1 -Version 1.2.3     # tests, publishes, packs; output in Releases\
```

Same steps CI runs, minus the upload. Running it again with a higher version against the same
`Releases\` directory produces a delta against what's already there, which is how the delta path gets
exercised without publishing anything.

`publish\` and `Releases\` are both gitignored.

## How updates reach users

`UpdateService` (in `src/BertBrowser.App/Services`) checks GitHub Releases on startup, on a
background thread, and swallows every failure — a broken check must never take down the app, and the
next launch retries. Behaviour worth remembering:

- **Updates are mandatory.** A newer release is downloaded and staged with
  `WaitExitThenApplyUpdates`, so it applies when the app closes whether or not the user takes the
  "Restart now?" prompt.
- **Pre-releases are ignored** (`prerelease: false`), so a GitHub pre-release is a safe way to stage
  something without pushing it to everyone.
- **Dev builds never update.** `_manager.IsInstalled` is false under `dotnet run`, so the check
  returns immediately.
- **This is the app's only network access**, which the README promises — keep it that way.

To exercise the real update flow against a local build, point the *installed* app at your
`Releases\` directory:

```powershell
$env:BERTBROWSER_UPDATE_URL = "C:\Source\bertbrowser\Releases"
```

Any static file host works as the value; it's read once, in the `UpdateService` constructor.

### Data must survive an update

The Velopack install directory is `%LOCALAPPDATA%\BertBrowser`, and **the installer deletes it** on
install and uninstall. So user data lives in `%USERPROFILE%\.bertbrowser\` (`bertbrowser.db`,
`settings.json`, `themes\`) and never in the install directory. `AppPaths.MigrateLegacyData` moves
data left behind by builds that got this wrong; don't remove it, and don't add anything new under
`%LOCALAPPDATA%`.

## winget

The package is `RobWalsh.BertBrowser`, published in
[`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs). A copy of what was submitted
lives in `manifests/` in this repo.

**winget is not automated, and it is not current.** Only 1.0.0 is published upstream. Because
installed copies update themselves, winget lag only affects first-time installs — but
`winget install RobWalsh.BertBrowser` hands new users an old build until a manifest is submitted for
the new version.

Two known problems with the published 1.0.0 manifest, worth fixing when the next version is
submitted:

- `InstallerType` is **`portable`**, pointing at the Velopack `Setup.exe`. It should be `exe`, with
  `Silent: --silent`, `Scope: user`, and `AppsAndFeaturesEntries` matching the ARP entry Velopack
  writes (DisplayName `BertBrowser`, Publisher `Rob Walsh`) — otherwise winget shims the installer
  as if it were the app, and upgrade/uninstall don't line up with what's actually installed.
- It has no `AppsAndFeaturesEntries`, so `winget upgrade` can't match the installed version.

Submitting a new version by hand:

```powershell
wingetcreate update RobWalsh.BertBrowser --version 1.2.3 `
  --urls https://github.com/robgwalsh/bertbrowser/releases/download/v1.2.3/BertBrowser-win-Setup.exe `
  --submit
```

To automate it instead, add this job to `.github/workflows/release.yml`:

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

Two prerequisites for that job:

1. A fork of `microsoft/winget-pkgs` named exactly `winget-pkgs` under your account — the action
   pushes manifest branches to it. The first `wingetcreate` submission created this fork already;
   otherwise `gh repo fork microsoft/winget-pkgs --clone=false`.
2. A **classic** PAT with only the `public_repo` scope — fine-grained PATs can't open PRs against
   repos you don't own. Create one
   [here](https://github.com/settings/tokens/new?scopes=public_repo&description=winget-releaser),
   then `gh secret set WINGET_PAT --repo robgwalsh/bertbrowser`.

## Release checklist

- [ ] `main` is green: `dotnet test bertbrowser.sln -c Release`
- [ ] Working tree clean, `main` pushed
- [ ] Pick the version (semver against the last tag)
- [ ] `git tag vX.Y.Z && git push origin vX.Y.Z`
- [ ] Workflow succeeds; release has six assets and a small delta
- [ ] Release notes written and attached
- [ ] Install or upgrade a real copy and launch it
- [ ] Submit the winget manifest, if you're keeping it current
