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
preview-fixture [dir]       files that really are what their extension says (a PNG with alpha, a
                            zip, C# and Markdown), in a Preview folder of their own — `tree`'s
                            photo.jpg is text, which previews as nothing
archive-fixture [dir]       the containers nothing here can write: locked.zip (AES, password
                            hunter2), sealed.7z (encrypted headers, correct-horse) and plain.7z.
                            Base64 in Core so a script and a unit test see the same bytes
mkdir <rel> | write <rel> [bytes] | sandbox
deny <rel>                  a file the current account may not delete or move — a real Deny ACE,
                            set with no privilege, lifted again on the way out. Its folder is
                            denied too and inheritably, so give it one of its own

go <path>                   navigate the active tab (relative paths resolve in the sandbox)
up | back | forward | refresh
enter <name>                open a folder row, as double-click does
tree-click <path>           click a folder in the sidebar tree (it must be showing already)
tree-expand <path> [on|off] toggle a row's chevron without selecting it (default on) — unlike
                            tree-click this doesn't navigate, so it won't promote/collapse siblings;
                            the only way to get two drive/device roots expanded at once

select <name>[, <name>…] | select-all | deselect
marquee <x0,y0> <x1,y1>      poses the rubber-band selection box over that rectangle (list
                            coordinates) and leaves it there for a `shot` — like the columns
                            page's drag line, it is only ever on screen mid-drag and a run posts
                            no mouse input

newtab [path] | closetab | reopen | tab <n>
movetab <from> <to>         put tab <from> in slot <to> (1-indexed), through the same
                            PaneViewModel.MoveTab a drop on the strip reports to. The drag is
                            mouse capture, which a run cannot post; `state`'s tabTitles and
                            `tab <n>` + `assert-path` check where the tabs ended up
tab-dragging <gap>          poses the strip's insertion line at a gap (0 is before the first tab)
                            and leaves it there for a `shot`, like `settings-columns-dragging`
split right|down [path] | closepane | pane <n>

search <text>               this folder's box, debounce and search waited out
gsearch <text>              the header's whole-PC box (needs --index)
clear-search

save-search <name> [current|folder|pc]
                            keep the search on show under a name, as the dialog's Save does —
                            seeded, validated and stored through the same code, dialog skipped
                            because a run never clicks. No scope word means the seed's default:
                            pc from the header box, current from the tab's. Re-saving a taken
                            name replaces that row
run-saved <name> [newtab]   run one as a click on its sidebar row does (or its "Run in new tab"),
                            debounce and search waited out. A pc-scoped one needs --index, like
                            gsearch
rename-saved <old> to <new> the edit dialog's rename, through SaveSearchAsync(previousName)
remove-saved <name>
saved-searches              the names on the sidebar, each with its scope

newfolder <name>            create a folder in the active tab's directory, through
                            PlanNewItem/CreateNewItemAsync (acts on the folder, not the selection)
newfile <name>              same for a file; a name ending in a configured type's extension
                            picks up that type's template, otherwise the file starts empty
rename <pattern>            the selection, through PlanRename/RenameAsync — the plain box, so the
                            pattern is literal and braces stay braces
rename-rule <key=value ...> the same, through the dialog's options panel. Quote a value with a
                            space in it. template= find= replace= regex=on matchcase=on
                            scope=stem|extension|wholename case=lower|upper|title|sentence
                            start=<n> step=<n>. Tokens: {name} {base} {ext} {parent} {n} {n:000}
                            {modified} {modified:<format>}, and {{ for a literal brace
delete | delete-permanent [names]
                            inside an archive, delete rewrites the container without those entries
                            (and Ctrl+Z puts the whole original back); rename does the same for one
move|copy [names] to <folder>
extract [names] to <folder> pull entries out of the archive on show, through the same planner,
                            executor and progress surface the menu uses
compress <format> <name>    zip | tar | tar.gz | tar.bz2 — the selection, or the folder on show
unlock <password>           give the archive on show a password and reload; writes the session
                            store directly, because the harness never clicks
undo

compare                     compare the two open panes, as F7 does, and wait for the scan. There
                            must be exactly two — with any other number the app refuses and says
                            why. Refuses to start a second comparison, because the command is a
                            toggle and a stray `compare` would quietly stop the first
compare-refused [text]      the other half: assert the pair was turned down (not exactly two panes,
                            an archive interior, a folder containing the other side) and that the
                            user was told. `text` is checked against what the app said. It says it
                            in a modal, which a run cannot dismiss, so the notice service is
                            recorded instead — which is also what makes the wording testable
compare-filter on|off       "show only differences", on both panes at once
compare-end                 stop comparing and clear the colours
sync [with-deletes]         run what the comparison would do, through the same planner, runner and
                            undo slot the dialog's Sync button uses — the dialog is skipped,
                            because a run never clicks. `with-deletes` ticks the destructive half,
                            which the dialog leaves off. `undo` reverses the whole thing

duplicates [folder]         scan for byte-identical files, defaulting to the folder on show. It
                            crawls the folder into fs_entry first — a harness run is unelevated, so
                            the MFT pass indexes nothing and there is no shortlist otherwise — and
                            awaits the scan, because `settle` does not know about it
duplicates-keep <strategy>  tick every copy but one: newest, oldest or shallowest
duplicates-remove           delete the ticked copies through PlanDelete/DeleteAsync, so they land
                            in the Recycle Bin and `undo` puts them back

changes-seed                write a dozen recorded file changes under the sandbox through the same
                            repository the index helper writes through (coalesced ×N rows, a rename
                            with its old name, a hidden entry, one too old for the hour, one past
                            retention), and turn the run's recording setting on. A run is
                            unelevated, so this is the only way `dialog changes` shows rows

hidden on|off | thumbnails <0..1> | sort <column-id> | theme <id>
                            (sort takes any catalogue id: Name, Size, Type, Modified, Created,
                            Accessed, Extension, or a canonical name such as
                            System.Image.Dimensions. "date" still means Modified.)
drives-view tree|cards      the "DRIVES & DEVICES" sidebar section's layout — what clicking its
                            header toggle button does
tree-scroll <px>            scrolls the sidebar's folder tree to a vertical offset, in pixels —
                            the only way to exercise PinnedRow/PinnedRootRow's scroll-driven
                            sticky headers, since nothing here synthesises mouse-wheel input
                            (PinnedRow itself is Depth-aware: it renders as the drive/device tile
                            style when pinning a root browsed directly, not just PinnedRootRow)
preview on|off              the active tab's preview pane, with its debounce and off-thread read
                            waited out (so assert after this, not straight after a `select`)
preview-mode auto|raw|hex   the pane's view override; sticky across selections, settled the same
                            way (`raw` is PreviewMode.Text — spelled the way the button is)

shot <name> [element]       PNG of the window, or of any x:Name'd element in it
dialog <kind> [name]        PNG of a dialog: new-folder, new-file, rename, rename-advanced,
                            delete, delete-permanent, message, warning, properties, settings,
                            theme-editor, disk-usage, duplicates, changes, sync-preview,
                            sync-preview-running, search-syntax, saved-search, extract, compress,
                            archive-password, elevation, settings-columns, settings-history,
                            columns, settings-columns-dragging
                            (changes is the "What changed" window: with a run's default settings
                            it shows the recording-off banner — the state every fresh install
                            has — and after `changes-seed` it shows rows. settings-history is
                            the page with its switch)
                            (both sync ones need a `compare` first, like `dialog duplicates`: they
                            show what that comparison found rather than starting one of their own.
                            sync-preview-running is the same window once Sync has been pressed —
                            the list read-only, and a bar and Cancel where the buttons were. Posed,
                            like `dialog transfer`, because it is a state that only exists while
                            something slow is happening)
                            (settings opens on General, so settings-columns is how the Columns
                            page gets photographed at all; it shows the *saved default*, so put an
                            arrangement in front of it with `columns default` first.
                            `columns` is the Add-column list, which the app shows in a Popup —
                            not a Window — so the harness hosts it in a bare one to photograph it)
                            (settings-columns-dragging is that page with a row being dragged: the
                            insertion line is placed, not dragged, because a run posts no mouse input)
                            (new-folder/new-file/search-syntax need no selection; every other
                            kind uses one.
                            rename-advanced is the rename dialog with its options panel open —
                            the panel is opened by a click, and this never clicks)
state                       one JSON line of everything worth asserting on
session                     save the pane/tab arrangement the way closing the window does,
                            prune it the way a launch does, and reopen it in place — assert on
                            what came back with assert-panes / assert-tabs / assert-path
rows                        the row names, for when an assertion is about to fail
columns                     the live GridView's columns as id:width. assert-visible cannot see a
                            GridViewColumn — it is not a FrameworkElement and never enters the
                            visual tree — so this is the only way to check one
columns add|remove <id>     edit the active tab's columns through ColumnLayoutRules, the same
columns move <id> <index>   functions the header menu and a header drag go through
columns width <id> <px>
columns reset               back to the saved default
columns default             the header menu's "Set as default for new tabs" — what the settings
                            page reads, and the only way to seed it from a script
menu columns [name]         PNG of the column header menu's items. They are rendered detached, not
                            opened: a ContextMenu is a Popup with its own top-level window that WPF
                            repositions onto the nearest monitor, i.e. onto the user's screen
probe <token> [element]     where a theme token's colour came out: the resolver, the app and
                            window resources, and the element's own Background/Foreground

assert-path <substring> | assert-status <substring> | assert-count <n>
assert-error [substring]     the warning banner above the list; bare = assert there is none
assert-row <name> | assert-no-row <name> | assert-selected <n>
assert-tabs <n> | assert-panes <n> | assert-flattened | assert-not-flattened
assert-inside-archive | assert-not-inside-archive
assert-column <id> | assert-no-column <id>
assert-columns <id>, <id>, ...            the whole column order, injected ones included
assert-metadata <row> <canonical> <text>  what a shell-metadata cell actually reads (substring)
assert-header-menu columns|files          which menu a right-click past the last column opens
assert-can-undo | assert-cannot-undo | assert-exists <path> | assert-missing <path>
assert-compare <substring>                the comparison banner's summary
assert-compare-row <row> <status>         a row's compare state, by the words its Status column
                                          shows. Read off the row, never off a screenshot: the
                                          tints are faint by necessity, so a pixel test would be
                                          asserting about the theme rather than about the verdict
assert-duplicate-groups <n> | assert-duplicate-selected <n>
assert-duplicate-row <name> | assert-no-duplicate-row <name>
assert-visible <Name> | assert-hidden <Name> | assert-not-launched
assert-elevation-offered | assert-no-elevation-offered
                            whether the run offered to retry something as administrator. The
                            negative form only means anything before the first offer — the
                            recording prompt accumulates over a run
assert-preview <kind>       image | document | text | hex | archive | font | media | loading | none

echo <text> | sleep <ms> | settle [ms]      '#' at the start of a line is a comment
```

Options: `--out <dir>` · `--sandbox <dir>` · `--state-dir <dir>` · `--keep-state` ·
`--allow-outside` · `--size WxH` · `--theme <id>` · `--start <path>` · `--index` ·
`--timeout <sec>` · `--busy-timeout <ms>` · `--keep-going` · `--verbose`

Element names come from the XAML: window-level are `FolderTree`, `GlobalSearchBox`, `PinnedRow`,
`ThumbSlider`, `PaneHostSite`; per tab (resolved against the *active* tab) are `FileListView`,
`SearchBox`, `PathBox`, `Breadcrumb`, `DetailsView`, `PreviewPane`; per pane, `TabHost` and
`ClosePaneButton`.

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
- **No UAC prompt is ever raised.** A file operation Windows refuses can now be retried with an
  administrator token, and a prompt takes the *secure desktop* — the one thing parking the window
  offscreen cannot work around. The run gets `RefusingElevationLauncher`, and `UiSession` asserts it
  did rather than trusting the registration. Everything above the launcher is real, so the
  discriminator, the rules and the merge are genuinely exercised; only the process and the token are
  missing. `dialog elevation` poses the consent window.
- **A modal the shell raises is recorded, not shown.** `IUserNotice` is how the shell says
  something that must be acknowledged — so far only "comparing needs two panes". The real one opens
  a `MessageDialog`, which a run could never dismiss; a run gets one that writes the message down
  for `compare-refused` to check, which is also what makes the wording itself testable.
- **No media is ever played.** The preview pane stops at a poster frame until someone presses play,
  and no script presses it. That keeps a run silent on a machine someone is using — and a
  `MediaElement` renders through its own composition surface, so it would come back as a hole in a
  `RenderTargetBitmap` anyway. Assert media through `state`, never a screenshot.
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
