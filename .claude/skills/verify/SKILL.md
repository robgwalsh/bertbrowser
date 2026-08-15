---
name: verify
description: Run, drive, and screenshot the BertBrowser WPF app without it appearing on the user's screen or taking their keyboard focus. Use whenever asked to run the app, start it, launch it, screenshot it, click through it, check a UI change, or confirm something works in the real interface. Never run `dotnet run` on src/BertBrowser.App — that opens a real window over whatever the user is doing.
---

# Driving BertBrowser's interface

The user multitasks while you work. A window that pops up over what they are doing and takes the
keyboard is not a minor annoyance — their keystrokes and yours end up in the same input queue. So
the app is never launched directly. Instead the harness hosts the same `MainWindow`, built from the
same composition root, in the same process, parked at -32000,-32000 and refused activation, and you
look at PNGs of it.

## Do not

- `dotnet run --project src\BertBrowser.App`
- run `BertBrowser.exe`
- synthesise keystrokes or mouse input with any tool

The only exception is the user explicitly asking you to launch the app.

## Pick the cheapest tier first

**Is the question about a rule?** What a move refuses, how a numbered rename lands, which items a
delete protects, how a path is canonicalised, what the planner does with a junction — none of that
needs a window. Write a test in `tests/BertBrowser.Core.Tests`; `TransferPlannerTests`,
`RenamePlannerTests` and `DeletePlannerTests` all run against fake filesystems, and the executor
tests against real files under `%TEMP%`. Deterministic, fast, no UI.

**Is the question about the interface?** Layout, theming, what a dialog looks like, whether the tree
reveals, what the status bar says, how tabs and panes arrange — that is the harness.

## Running it

```powershell
$harness = "C:\Source\bertbrowser\tools\BertBrowser.Harness\bin\Debug\net10.0-windows\BertBrowser.Harness.exe"
```

Build with `dotnet build C:\Source\bertbrowser\bertbrowser.sln` first. If the build fails with
MSB3021 because a running BertBrowser locks `bin\Debug`, kill it
(`Get-Process BertBrowser | Stop-Process -Force`) and rebuild.

```powershell
& $harness --script C:\Source\bertbrowser\tools\ui\smoke.bbs          # the canonical pass
& $harness --script C:\Source\bertbrowser\tools\ui\themes.bbs         # every built-in theme + dialogs
& $harness -c "tree .; refresh; shot check" --out $env:TEMP\look      # ad hoc

# The folder tree keeping its selection through a rebuild. Needs a visible sandbox — see the
# last bullet under "What to trust".
& $harness --script C:\Source\bertbrowser\tools\ui\tree.bbs `
    --sandbox C:\Source\bertbrowser-treecheck --allow-outside
```

Then `Read` each path printed after `SHOT `.

Exit codes: `0` pass · `1` a command or assertion failed · `2` environment · `3` watchdog.

## Commands

```
tree [dir]                  lay down a throwaway fixture tree (folders, hidden entries, files of
                            several types, all stamped with one modified date)
mkdir <rel> | write <rel> [bytes] | sandbox

go <path>                   navigate the active tab (relative paths resolve in the sandbox)
up | back | forward | refresh
enter <name>                open a folder row, as double-click does
tree-click <path>           click a folder in the sidebar tree (it must be showing already)

select <name>[, <name>…] | select-all | deselect

newtab [path] | closetab | tab <n>
split right|down [path] | closepane | pane <n>

search <text>               this folder's box, debounce and search waited out
gsearch <text>              the header's whole-PC box (needs --index)
clear-search

rename <pattern>            the selection, through PlanRename/RenameAsync
delete | delete-permanent [names]
move|copy [names] to <folder>
undo

hidden on|off | thumbnails <0..1> | sort name|size|modified|type | theme <id>

shot <name> [element]       PNG of the window, or of any x:Name'd element in it
dialog <kind> [name]        PNG of a dialog: rename, delete, delete-permanent, message, warning,
                            properties, settings, theme-editor
state                       one JSON line of everything worth asserting on
rows                        the row names, for when an assertion is about to fail
probe <token> [element]     where a theme token's colour came out: the resolver, the app and
                            window resources, and the element's own Background/Foreground

assert-path <substring> | assert-status <substring> | assert-count <n>
assert-row <name> | assert-no-row <name> | assert-selected <n>
assert-tabs <n> | assert-panes <n> | assert-flattened | assert-not-flattened
assert-can-undo | assert-cannot-undo | assert-exists <path> | assert-missing <path>
assert-visible <Name> | assert-hidden <Name> | assert-not-launched

echo <text> | sleep <ms> | settle [ms]      '#' at the start of a line is a comment
```

Options: `--out <dir>` · `--sandbox <dir>` · `--state-dir <dir>` · `--keep-state` ·
`--allow-outside` · `--size WxH` · `--theme <id>` · `--start <path>` · `--index` ·
`--timeout <sec>` · `--busy-timeout <ms>` · `--keep-going` · `--verbose`

Element names come from the XAML: window-level are `FolderTree`, `GlobalSearchBox`, `PinnedRow`,
`ThumbSlider`, `PaneHostSite`; per tab (resolved against the *active* tab) are `FileListView`,
`SearchBox`, `PathBox`, `Breadcrumb`, `DetailsView`; per pane, `TabHost` and `ClosePaneButton`.

## Blocked on purpose

- **Nothing is launched.** The window is given an `IProcessLauncher` that refuses, because opening a
  file starts whatever program owns it and *that* window lands on the user's desktop. So `enter` on
  a file is refused, and "Open in Terminal", custom commands and the portable-device handler all
  report rather than run. `assert-not-launched` proves a gesture did not even try.
- **The clipboard is not touched.** There is one per session and the user is using it. `move` and
  `copy` go through `TransferPlanner`/`TransferExecutor` — the same code paste and drag-and-drop go
  through — without it.
- **Writing outside the sandbox is refused.** This app deletes and moves real files, and the harness
  drives the real executors. `--allow-outside` is the deliberate way past, for when you mean it.
- **The MFT indexer does not run** unless you pass `--index`. It reads every NTFS volume's master
  file table — minutes of disk on a machine someone is using — and needs administrator rights the
  harness does not request. `gsearch` says so rather than reporting an empty result as a finding.

## Recipes

**Look at a change you just made**

```powershell
& $harness -c "tree .; refresh; shot after-change" --out $env:TEMP\look
```

**A dialog, in both themes**

```powershell
& $harness -c "tree .; refresh; select notes.txt, report.md; dialog rename dark; theme light-plus; dialog rename light"
```

**Prove a move and its undo really happened on disk**

```powershell
& $harness -c "tree .; refresh; select photo.jpg; move to Documents; assert-exists Documents/photo.jpg; undo; assert-missing Documents/photo.jpg; assert-exists photo.jpg"
```

## What to trust

- Captures are a software re-render of the visual tree, not a screen grab, so being offscreen costs
  nothing and a fullscreen game in front of the window changes nothing. A `shot` that comes back one
  flat colour fails rather than lying to you.
- **Do not diff UI screenshots pixel for pixel** — text rasterisation varies with font version and
  DPI. Assert through `state`, `assert-status`, `assert-count`, `assert-row`.
- **The fixture is deterministic** — same names, same byte counts, one fixed modified date — so two
  captures of the same script differ only where the change under test made them differ. Anything
  outside it is not: the folder tree shows the real machine's drives.
- Each run gets a scratch `BERTBROWSER_DATA_DIR`, so it cannot read or corrupt the user's real
  search index, settings or themes, and it is deleted afterwards. Use `--keep-state` only when you
  mean to test something across two runs.
- **A run reporting more than one foreground correction means something new is activating the
  window** — worth investigating rather than ignoring. One is normal: WPF makes the window
  foreground once during the first layout pass through no event this process is told about, and
  `ForegroundGuard` hands it straight back within ~10 ms. The count is in `state`.
- `sandbox` paths under `%TEMP%` sit inside `AppData`, which is hidden — so the folder tree cannot
  reveal down to them with "Show hidden items" off. Use `--sandbox` somewhere visible if a capture
  is about the tree.
