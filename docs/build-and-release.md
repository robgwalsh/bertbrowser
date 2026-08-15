# Build and release

How BertBrowser is built, packaged, and shipped. The short version: **push a tag, CI does the
rest.** Everything below is the detail behind that.

## Prerequisites

| For | Install |
|---|---|
| Building and testing | [.NET 10 SDK](https://dotnet.microsoft.com/download/dotnet/10.0) |
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
  building, testing and packaging do not, and neither do the programs it launches (see
  [SECURITY.md](../SECURITY.md) — children go out through the desktop shell at the user's own
  integrity level).

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

A second job then opens the [winget](#winget) pull request for the version just published.

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

## The unstable channel

`main` also ships. `.github/workflows/unstable.yml` runs on **every push to `main`** (and on
`workflow_dispatch`), and does what the release workflow does with four differences: the version, the
channel, the release it publishes to, and no winget.

| | Release | Unstable |
|---|---|---|
| Trigger | push tag `v*` | push to `main` |
| Version | the tag, minus `v` | last tag + `0.0.1`, plus `-unstable.<run number>` |
| Velopack channel | `win` (the default) | `unstable` |
| Feed file | `releases.win.json` | `releases.unstable.json` |
| Installer | `BertBrowser-win-Setup.exe` | `BertBrowser-unstable-Setup.exe` |
| GitHub release | one per version, tag `vX.Y.Z` | one rolling **pre-release**, tag `unstable` |
| Deltas | yes | yes |
| winget | yes | no |

**The two cannot reach each other**, which is the property worth protecting, and it is guarded twice
over. A release copy passes `prerelease: false`, so the unstable pre-release is not even in the list
of releases it considers; and Velopack asks each release in that list for
`releases.<channel>.json` and **skips any release that hasn't got one**, so the feeds would not cross
even if a release did turn up. That second guard is what makes the reverse direction safe too — an
unstable copy lists stable releases and skips every one of them, rather than being confused by a
newer stable release. On top of that, GitHub's `latest` excludes pre-releases, so the README's stable
download links never resolve to an unstable build, and this workflow has no winget job, so nothing
off `main` is ever offered to winget.

`1.1.3-unstable.42` reads as "heading for 1.1.3, build 42". It sorts *below* `1.1.3` — a SemVer
pre-release always does — and above `1.1.3-unstable.7`, because SemVer compares dot-separated numeric
identifiers numerically. That is why the run number is a separate identifier rather than glued on.

Things about the **rolling tag** worth knowing before changing any of it:

- **The release is deleted and recreated on every run**, rather than merged into: `vpk upload`
  refuses to add a second `releases.unstable.json` to a release that already has one, and GitHub
  ignores `target_commitish` for a tag that already exists — so the tag has to be deleted with it or
  the release keeps pointing at the old commit. The download URL 404s for the couple of minutes an
  upload takes; a copy that checks in that window finds no feed and quietly does nothing.
- **Deletion goes by release id, not by tag, and pushes queue rather than cancel.** Both guard the
  same failure: a run interrupted mid-upload strands a *draft* release holding the `unstable` tag.
  A draft has no tag, so `gh release delete unstable` cannot see it — while vpk's own collision check
  can, and refuses to publish. Left alone that wedges the channel until someone clears it by hand.
- **Deltas still work**, and matter more here than on a release, since an unstable copy updates on
  every push. The delta is built at pack time against the package `vpk download` pulls from the
  release that is still up, and a client applies it against its own local package — neither has
  anything to do with the release being replaced afterwards. `--pre` on that download is what makes
  it find anything at all, the feed living on a pre-release; without it the delta silently comes out
  full-sized.
- **The fixed tag is the only way the README can link to "the current unstable build".** There is no
  `releases/latest/download/...` form for a pre-release.

The badge is the fiddly part, and two obvious approaches are both dead ends. `github/v/release` with
`?include_prereleases` renders the **tag name** — for a rolling tag that is the constant string
`unstable`, no version in it at all (neovim's `nightly` badge reads exactly that way). And shields'
`endpoint` route **blocks `github.com` outright**, so serving it a `badge.json` uploaded as a release
asset renders `domain is blocked`, permanently. What works is `dynamic/json` against
`api.github.com`, reading the release's `name` — which is why the workflow names the release the bare
version rather than `BertBrowser <version>`. That route is unauthenticated and so in principle
rate-limitable; the failure mode is a badge briefly reading `invalid`, and the build-status badge
beside it never depends on GitHub's API at all.

### The app knows which channel it is on

`ReleaseChannel.IsUnstable` (in Core, with tests) reads it off the build's own version string, and
`UpdateService` uses it for the one flag that differs — whether `GithubSource` looks at pre-releases.
Both directions are load-bearing: an unstable build that refused pre-releases could never see its own
feed, and a release build that accepted them would be handed the unstable pre-release, which carries
no `releases.win.json`. The version is also what the title bar shows, so which channel a copy is on is
visible without digging.

**Unstable replaces a release install rather than sitting beside it** — same `packId`, so same install
directory. That is deliberate: two copies would share `%USERPROFILE%\.bertbrowser` and the
single-instance mutex and fight over the database. Running the release `Setup.exe` over an unstable
install puts it back on the release channel.

## Build an installer locally

```powershell
scripts\pack.ps1 -Version 1.2.3     # tests, publishes, packs; output in Releases\
```

Same steps CI runs, minus the upload. Running it again with a higher version against the same
`Releases\` directory produces a delta against what's already there, which is how the delta path gets
exercised without publishing anything.

`publish\` and `Releases\` are both gitignored. Note the rule is `[Rr]eleases/` with no leading slash,
so it matches a directory of that name **anywhere** in the tree — a source folder called `Releases\`
would be silently untracked and would build locally right up until CI checked out without it. That is
why the channel predicate lives in `src\BertBrowser.Core\Updates\`.

`pack.ps1` packs the release channel. To exercise the unstable one, pass `vpk` the two extra
arguments CI passes — a pre-release version and the channel:

```powershell
vpk pack --packId BertBrowser --packVersion 1.1.3-unstable.42 --packDir publish `
  --mainExe BertBrowser.exe --channel unstable --delta None
```

That is also how to check the channel plumbing without publishing anything: pack `-unstable.1` and
`-unstable.2` into a `Releases\` directory, install the first, point `BERTBROWSER_UPDATE_URL` at that
directory, and confirm it finds the second.

## How updates reach users

`UpdateService` (in `src/BertBrowser.App/Services`) checks GitHub Releases on startup, on a
background thread, and swallows every failure — a broken check must never take down the app, and the
next launch retries. Behaviour worth remembering:

- **Updates are mandatory.** A newer release is downloaded and staged with
  `WaitExitThenApplyUpdates`, so it applies when the app closes whether or not the user takes the
  "Restart now?" prompt.
- **A release build ignores pre-releases**, an unstable build reads them — that one flag is the whole
  difference, and it comes from the build's own version (see [The unstable
  channel](#the-unstable-channel)). Note the consequence: a GitHub pre-release is **no longer** a
  quiet way to stage something, because the rolling `unstable` pre-release is what every unstable
  copy is watching. Use a draft release for that.
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

**This is automated.** The `winget` job in `release.yml` runs after the release is published and
opens a pull request at `microsoft/winget-pkgs` for the new version. Nothing to do by hand; the PR
is merged by winget's own validation pipeline, usually within a few hours.

The job needs two things, both already set up:

1. A fork of `microsoft/winget-pkgs` named exactly `winget-pkgs` under the account — the action
   pushes manifest branches to it. Recreate with `gh repo fork microsoft/winget-pkgs --clone=false`.
2. A **classic** PAT with only the `public_repo` scope, as the `WINGET_PAT` secret — fine-grained
   PATs can't open PRs against repos you don't own. Create one
   [here](https://github.com/settings/tokens/new?scopes=public_repo&description=winget-releaser),
   then `gh secret set WINGET_PAT --repo robgwalsh/bertbrowser`. **A classic PAT expires**, and the
   symptom is this job failing on a release that otherwise went fine.

A failure here never affects the release — the assets are already published and installed copies
update themselves regardless. `gh run rerun <id>` is usually enough; winget lag only delays
first-time installers.

### The upstream manifest is the template for the next one

The action generates each version's manifest from **the newest one already published upstream**,
changing the version, URL and hash. So anything wrong up there propagates forward instead of being
corrected, and anything worth having has to be put there once, by hand.

That was done for 1.1.1, which is the baseline the automation now copies. The 1.0.0 manifest
described the Velopack `Setup.exe` as `InstallerType: portable` — which makes winget shim the
installer as if it were the app — and carried no `AppsAndFeaturesEntries`, so `winget upgrade`
couldn't match an installed copy. 1.1.1 fixes both:

- `InstallerType: exe`, `Scope: user` (per-user install into `%LOCALAPPDATA%`, no elevation),
  `InstallerSwitches.Silent: --silent` (Velopack's flag), `UpgradeBehavior: install`.
- `AppsAndFeaturesEntries` with `ProductCode: BertBrowser` — the HKCU uninstall key Velopack writes —
  plus DisplayName `BertBrowser` and Publisher `Rob Walsh`. **No `DisplayVersion`**, deliberately:
  Velopack's ARP version always equals the package version, so leaving it out lets winget compare
  against `PackageVersion` rather than a field that would go stale on every automated update.
- A filled-in locale manifest (publisher/package URLs, license URL, description, tags).

A copy of each hand-authored submission lives in `manifests/` in this repo. It is a record, not a
source — the automation reads the upstream copy. To change the published metadata, edit a manifest
by hand and submit it as above; the next automated release then inherits the change.

Submitting a version by hand (if the job is broken, or to change metadata):

```powershell
wingetcreate update RobWalsh.BertBrowser --version 1.2.3 `
  --urls https://github.com/robgwalsh/bertbrowser/releases/download/v1.2.3/BertBrowser-win-Setup.exe `
  --submit
```

Validate anything hand-written before submitting — `winget validate --manifest <dir>` catches schema
errors that would otherwise come back as a failed check on the PR.

## Release checklist

- [ ] `main` is green: `dotnet test bertbrowser.sln -c Release`
- [ ] Working tree clean, `main` pushed
- [ ] Pick the version (semver against the last tag)
- [ ] `git tag vX.Y.Z && git push origin vX.Y.Z`
- [ ] Both jobs succeed; release has six assets and a small delta
- [ ] Release notes written and attached
- [ ] Install or upgrade a real copy and launch it
- [ ] The winget PR is open at `microsoft/winget-pkgs` (the `winget` job opens it; nothing to do
      unless it failed)
