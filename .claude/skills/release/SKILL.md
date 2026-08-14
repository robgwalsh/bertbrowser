---
name: release
description: Ship a new BertBrowser version — pre-flight checks, tag, watch CI, verify assets, write release notes, confirm the winget PR. Use when asked to release, ship, cut, or publish a version.
---

# Releasing BertBrowser

**The tag is the release.** No file carries a version; CI derives it from the tag, builds, packs
with Velopack, publishes the GitHub Release, and opens the winget PR. Full detail lives in
[docs/build-and-release.md](../../../docs/build-and-release.md) — this is the procedure to follow.

Nothing here is reversible once the tag is pushed except by deleting the release, so do the
pre-flight first and do not skip it.

## 1. Pre-flight

```powershell
git fetch origin
git status -sb                      # clean tree, main in sync with origin/main
git rev-parse HEAD origin/main      # must match — the tag builds what's on the branch
dotnet test bertbrowser.sln -c Release
git tag --sort=-v:refname | head -3 # the last version, to pick the next one
```

A red test fails the release *after* the tag exists, which is the annoying case. Run it locally.

Pick the version with semver as a user reads it: features → minor, fixes only → patch.

## 2. Tag and push

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

Do not ask the user to confirm the version again if they named it; do ask if they said only
"release" and the right bump is ambiguous from the diff.

## 3. Watch CI

```powershell
gh run list --workflow=release.yml --limit 3     # grab the run id for the new tag
gh run watch <id> --exit-status --interval 15
```

Run the watch in the background and write the release notes (step 5) while it builds — the run takes
~2–3 minutes and the notes need reading the diff anyway.

## 4. Verify what shipped

```powershell
gh release view vX.Y.Z --json assets -q '.assets[] | "\(.name) \(.size)"'
```

A healthy release has **six** assets: `Setup.exe`, `Portable.zip`, `-full.nupkg`, `-delta.nupkg`,
`releases.win.json`, `RELEASES`. **The delta must be much smaller than the full package** (a few MB
vs ~75) — same size means `vpk download github` didn't find the previous release and installed
copies will take a full download.

## 5. Release notes

`vpk upload` publishes an empty body, so notes are always added afterwards:

```powershell
gh release edit vX.Y.Z --notes-file <file>
```

**The commit subjects in this repo are useless for this** — real ones include "some fixes", "not
useful", "security". Read the actual changes:

```powershell
git log --oneline vPREV..vX.Y.Z
git diff --stat vPREV..vX.Y.Z
git show <sha> -- src/BertBrowser.App/Views src/BertBrowser.App/ViewModels   # what the user sees
```

Write from what changed *for a user*, grouped by feature area, not by commit. Code comments in the
diff are usually the best source — this codebase explains *why* a fix was made right where it was
made, and that reasoning is what makes a note worth reading. Match the voice of
[v1.1.0](https://github.com/robgwalsh/bertbrowser/releases/tag/v1.1.0) and
[v1.1.2](https://github.com/robgwalsh/bertbrowser/releases/tag/v1.1.2): a one-line summary of the
release, then the update/install line, then `##` sections, then the compare link
(`https://github.com/robgwalsh/bertbrowser/compare/vPREV...vX.Y.Z`). Keep the notes file in the
scratchpad — it is not a repo artifact.

## 6. winget

The second job opens the PR at `microsoft/winget-pkgs` automatically:

```powershell
gh run view <id> --json jobs -q '.jobs[] | "\(.name) \(.conclusion)"'
```

A failure here never affects the release — assets are published and installed copies update anyway.
Usual cause is the **classic `WINGET_PAT` secret expiring** (that is what killed it on 1.1.2): the
log reads `GITHUB_TOKEN:` with nothing after it and `Error: GitHub token is invalid`, and
`gh secret list` still shows the secret, since an expired token is present but dead. The fix is the
user's to do — a new classic PAT with only `public_repo` from
<https://github.com/settings/tokens/new?scopes=public_repo&description=winget-releaser>, then
`gh secret set WINGET_PAT --repo robgwalsh/bertbrowser` and `gh run rerun <id> --job <winget-job>`.
Report it rather than hand-submitting unless they ask.

The action builds each manifest from **the newest one already upstream**, so metadata changes have
to be made once by hand upstream and are then inherited. `manifests/` in this repo is a record of
what was submitted, not a source — no need to update it for an automated release.

## If it goes wrong

Nothing published yet — fix, then re-tag the same version:

```powershell
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
```

Release already created — delete it too rather than leaving a half-uploaded one for installed apps
to find: `gh release delete vX.Y.Z --cleanup-tag`. For a transient failure, `gh run rerun <id>`.

## Report back

Tell the user: the version, the run result, the asset count and delta size, the notes URL, and the
winget job's outcome. Don't offer to install or launch the app to verify unless they ask.
