---
name: verify
description: Run, drive, and screenshot the BertBrowser app without it appearing on the user's screen or taking their keyboard focus. Use whenever asked to run the app, start it, launch it, screenshot it, click through it, check a UI change, or confirm something works in the real interface. Never run BertBrowser.exe - that opens a real window over whatever the user is doing.
---

# Driving BertBrowser's interface

The user multitasks while you work. A window that pops up over what they are doing and takes the
keyboard is not a minor annoyance - their keystrokes and yours end up in the same input queue. So
the app is never launched directly. Instead `bb-harness.exe` (`harness`) builds the same
window from the same C++ UI library and the same Rust core, in its own process, parked at
-32000,-32000, refused activation and disabled, rendering through WARP into an offscreen bitmap, and
you look at PNGs of it. It is a separate exe, never a mode of `BertBrowser.exe`: it links
`bbcore_harness.lib`, which carries the test fixtures and fakes the product must never contain.

## Do not

- run `BertBrowser.exe`
- synthesise keystrokes or mouse input with any tool

The only exceptions are the user explicitly asking you to launch the app, and `bb-bench startup`
(which parks its window offscreen against a scratch data directory).

## Pick the cheapest tier first

**Is the question about a rule?** What a move refuses, how a numbered rename lands, which items a
delete protects, how a path is canonicalised, what the planner does with a junction - none of that
needs a window. Write a test in `crates/bb-core/tests/all/` (`transfer_planner_tests.rs`,
`rename_planner_tests.rs`, `delete_planner_tests.rs` run against fake filesystems, the executor tests
against real files under `%TEMP%`). Run it with `cargo test -p bb-core --test all <filter>`, from
PowerShell. Deterministic, fast, no UI.

**Is the question about the interface?** Layout, theming, what a dialog looks like, whether the tree
reveals, what the status bar says, how tabs and panes arrange - that is the harness.

**Is the question about speed?** `target\release\bb-bench.exe run --filter *pathkey*` for a core
path, `... ui --scenario listing-10k` for what a user feels - not a stopwatch in a script. The harness's
`time` verb exists, but only `bb-bench` turns its samples into something a baseline can be compared
with. See `bench/README.md`.

## Running it

```powershell
$harness = "C:\Source\bertbrowser\out\debug\harness\bb-harness.exe"
```

Build with `cd C:\Source\bertbrowser; pwsh .\build.ps1` first (`-Config release` for
`out\release\...`). `build.ps1` prints a harmless "vswhere.exe not recognized" line. The harness links
the Rust core, so a Rust change needs the rebuild too, and the regenerated `include\bbcore.h` is to be
committed. If the link fails with LNK1104 on a running exe, kill it and rebuild.

**Give every run a fresh `--out` directory.** The default sandbox lives under it
(`<out>\sandbox`), so reusing one carries last run's tree into this one.

```powershell
$out = "$env:TEMP\look-$(Get-Date -Format HHmmss)"
& $harness --script C:\Source\bertbrowser\tools\ui\smoke.bbs --out $out          # the canonical pass
& $harness --script C:\Source\bertbrowser\tools\ui\themes.bbs --out $out-t       # every built-in theme + dialogs
& $harness --script C:\Source\bertbrowser\tools\ui\system-theme.bbs --out $out-s # matching the Windows theme
& $harness --script C:\Source\bertbrowser\tools\ui\transfer.bbs --out $out-x     # the queue, pause, per-item conflicts
& $harness --script C:\Source\bertbrowser\tools\ui\merge.bbs --out $out-m        # a folder merged into a folder of the same name
& $harness --script C:\Source\bertbrowser\tools\ui\keys.bbs --out $out-k         # which command a chord runs, and where it is live
& $harness --script C:\Source\bertbrowser\tools\ui\palette.bbs --out $out-p      # the command palette
& $harness --script C:\Source\bertbrowser\tools\ui\keyboard.bbs --out $out-kb    # the Keyboard settings page and its recorder
& $harness --script C:\Source\bertbrowser\tools\ui\commands.bbs --out $out-c     # selection, other-pane, view and sort commands
& $harness --script C:\Source\bertbrowser\tools\ui\command-bars.bbs --out $out-b # the menu bar, toolbars and their settings page
& $harness -c "tree .; refresh; shot check" --out $out-adhoc                     # ad hoc

# The folder tree keeping its selection through a rebuild. Needs a visible sandbox - see the
# last bullet under "What to trust".
& $harness --script C:\Source\bertbrowser\tools\ui\tree.bbs --out $out-tree `
    --sandbox C:\Source\bertbrowser-treecheck --allow-outside

# The degraded-index banner: poses a declined elevation prompt.
& $harness --script C:\Source\bertbrowser\tools\ui\index-degraded.bbs --index-declined --out $out-i
```

Then `Read` each path printed after `SHOT `. Stdout is the protocol: `OK <command>` per command,
`SHOT <path>`, `STATE {json}`, `BENCH`/`MEM` samples, `FAIL ...` on the first failure (unless
`--keep-going`). A verb whose feature is not built yet fails with "is not implemented in the native
harness yet".

Exit codes: `0` pass · `1` a command or assertion failed · `2` environment · `3` watchdog · `64` usage.
A crash prints `CRASH <code> at <module>+<offset>` and a symbolised stack, and writes `crash.dmp` into
`--out`.

## Commands

```
tree [dir]                  lay down a throwaway fixture tree (folders, hidden entries, files of
                            several types, all stamped with one modified date)
preview-fixture [dir]       files that really are what their extension says (a PNG with alpha, a
                            zip, C# and Markdown), in a Preview folder of their own — `tree`'s
                            photo.jpg is text, which previews as nothing
many-fixture [n] [dir]      n files (400 by default) across nested folders, every fourth one a .txt
                            so both tile shapes are present. For the cases only quantity shows:
                            the thumbnail view has to stay cheap over thousands of rows, and no
                            fixture small enough to name its rows can prove that
archive-fixture [dir]       the containers nothing here can write: locked.zip (AES, password
                            hunter2), sealed.7z (encrypted headers, correct-horse) and plain.7z.
                            Base64 in `bb-test-support` so a script and a unit test see the same bytes
mkdir <rel> | write <rel> [bytes] | sandbox
write-text <rel> <line>|<line>|…
                            a file with content you chose. `write` fills a file with its own name
                            repeated, which is right for hashing (deterministic, and two names
                            never collide) and wrong for a diff, where two such files share no line
                            at all. This is how a script makes a pair that differs in the middle
write-binary <rel> <bytes> [differ-at]
                            a file that is genuinely not text, so a comparison takes the hex path
                            rather than the line diff. A .bin full of repeated ASCII is not a
                            binary and the file-compare plan is right to say so. `differ-at` flips one
                            byte, which is what puts the first difference somewhere worth scrolling
checksum-file <rel> <algorithm> <name>,<name>…
                            a real .sfv/.md5/.sha256 over real files, written through the same
                            renderer the app's Save goes through
checksum-algorithms <a>,<a> which boxes the next checksum window opens with (Crc32, Md5, Sha1,
                            Sha256, Sha512) — the window reads them from settings exactly as it
                            does for a person
hide-menu-item <id> | show-menu-item <id>
                            untick (or re-tick) one of the app's own right-click entries on the
                            Context menu page, by the ids in the core's `built_in` menu items (copy-path,
                            open-terminal, cut…); the next `menu` reads it as a person's save would
command-bars reset | toolbar title|row<n> [text] <item…> | menu <Name> <item…>
command-bars save|apply|remove <name> | rename <old> to <new> | saved
                            arrange the menu bar and toolbars through `CommandBarService`, the app's
                            one writer for them. An item is a command id, `-`, or `@list`
                            (`@workspaces`); `text` labels the toolbar's buttons; a row past the end
                            is made. `apply` goes through the window, so it asks what a person is asked
menu-layout <token…> | menu-layout default
                            arrange both right-click menus as the Context menu page saves them:
                            app:<id>, -, cmd:<id>, clsid:{…}/verb:<name>, @more (MenuLayoutRules)
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
                            move a drop on the strip reports to. The drag is
                            mouse capture, which a run cannot post; `state`'s tabTitles and
                            `tab <n>` + `assert-path` check where the tabs ended up
tab-dragging <gap>          poses the strip's insertion line at a gap (0 is before the first tab)
                            and leaves it there for a `shot`, like `settings columns dragging`
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
rename-saved <old> to <new> the edit dialog's rename, through the same save the dialog uses
remove-saved <name>
saved-searches              the names on the sidebar, each with its scope
assert-bookmarks <a> | <b> | … | none
                            the sidebar's Bookmarks section, by name, in order - hidden targets
                            are left out while hidden items are, and shown dimmed otherwise

newfolder <name>            create a folder in the active tab's directory, through
                            the new-item planner and executor (acts on the folder, not the selection)
newfile <name>              same for a file; a name ending in a configured type's extension
                            picks up that type's template, otherwise the file starts empty
rename <pattern>            the selection, through the rename planner and executor — the plain box, so the
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
conflict <answer>            what the conflict dialog comes back with. skip | replace | keep-both |
conflict <name>=<answer>,…   overwrite | cancel for every clash, or one answer per name — the case
                             the single-answer dialog could not express. A run never clicks, so this
                             answers through the same conflict-prompt seam the shell asks on.
                             Defaults to keep-both for everything, which is what every transfer got
                             before there was a prompt, so scripts that say nothing are unaffected.
                             A name may be a path relative to the drop (`photos/2024/a.jpg`, either
                             slash) — after a merge two rows can share a leaf name
conflict-plan move|copy <names> to <folder>
                             builds (but does not run) a plan that clashes, for `dialog conflicts`
                             to photograph — the same arrangement `duplicates` and
                             `dialog duplicates` use. Expands folder-vs-folder clashes exactly as a
                             real drop does, so what it photographs is the merge, not one wholesale
                             row
pause | resume               hold the transfer queue at its next chunk boundary and let it go again,
                             through the same commands the buttons are bound to. A paused queue is
                             treated as settled: IsTransferring stays raised for a pause, so waiting
                             for it to drop would hang every run that photographs one
queue-move <from> <to>       reorder a waiting job, 1-indexed over the whole queue. The running job
queue-cancel <n>             is always first and cannot move or be jumped; cancelling a waiting one
                             drops it without ever running it
shortcut [names] to <folder> the right-drag menu's third verb: a .lnk per item, named Explorer's
                            way ("notes.txt - Shortcut.lnk", stepping aside to "(2)"). Goes through
                            the real IShellLink, which is the point — whether a link this app writes
                            is one Windows reads back is the one thing a core test cannot answer
extract [names] to <folder> pull entries out of the archive on show, through the same planner,
                            executor and progress surface the menu uses
compress <format> <name>    zip | tar | tar.gz | tar.bz2 — the selection, or the folder on show
unlock <password>           give the archive on show a password and reload; writes the session
                            store directly, because the harness never clicks
undo | redo                 one step through the undo history, as Ctrl+Z / Ctrl+Y
undo-to <n> | redo-to <n>   n steps at once, counted from the cursor — what picking the nth entry
                            of the Undo/Redo dropdown does. Stops at the first step that is not clean
undo-budget steps=<n> gb=<n> the History page's limits, either or both; lowering one releases
                            (and commits) the oldest entries straight away
clear-undo-history          the History window's Clear, confirmation skipped
assert-can-redo | assert-cannot-redo
assert-history <n> [cursor=<k>] [released=<r>]
                            entries held, how many are in effect, how many the limits let go
assert-history-state <n> done|undone|partial|failed
                            the nth newest entry, as its badge in the History window reads

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
settle-content              re-judge the selected rows by their bytes, through the same
                            content settlement the menu item uses, and await it. Identical bytes
                            raise Newer/Older/Unknown to Same; nothing else moves. Prints
                            "SETTLED <n>". Needs a `compare` first
compare-end                 stop comparing and clear the colours
sync [with-deletes]         run what the comparison would do, through the same planner, runner and
                            undo history the dialog's Sync button uses — the dialog is skipped,
                            because a run never clicks. `with-deletes` ticks the destructive half,
                            which the dialog leaves off. `undo` reverses the whole thing

duplicates [folder]         scan for byte-identical files, defaulting to the folder on show. It
                            crawls the folder into fs_entry first — a harness run is unelevated, so
                            the MFT pass indexes nothing and there is no shortlist otherwise — and
                            awaits the scan, because `settle` does not know about it
duplicates-keep <strategy>  tick every copy but one: newest, oldest or shallowest
duplicates-remove           delete the ticked copies through the delete planner and executor, so they land
                            in the Recycle Bin and `undo` puts them back

changes-seed                write a dozen recorded file changes under the sandbox through the same
                            repository the index helper writes through (coalesced ×N rows, a rename
                            with its old name, a hidden entry, one too old for the hour, one past
                            retention), and turn the run's recording setting on. A run is
                            unelevated, so this is the only way `dialog changes` shows rows

system-theme light|dark|high-contrast
                            poses what Windows is set to. A run never reads the real machine — it
                            starts posed dark and pinned, so every existing script and screenshot
                            is unaffected by the developer's own desktop
system-follow on|off        the "Match Windows light/dark" checkbox. Kept apart from `system-theme`
                            on purpose, so a script can pose a switch the app is ignoring
system-slot light|dark <id> the two slot pickers. Setting the slot for the mode Windows is *not* in
                            configures without previewing
hidden on|off | thumbnails <0..1> | sort <column-id> | theme <id>
                            (`theme` pins one, which is what stops following — the same as choosing
                            one by hand in Settings)
                            (sort takes any catalogue id: Name, Size, Type, Modified, Created,
                            Accessed, Extension, or a canonical name such as
                            System.Image.Dimensions. "date" still means Modified.)
workspaces-placement | saved-searches-placement sidebar|titlebar|hidden
                            where that list is offered, as its Settings page sets it
list-scroll <px>            scrolls the file list to a vertical offset, in pixels. Worth more than
                            tree-scroll: the thumbnail view lays tiles out by arithmetic in this app's
                            own `TileView`, so what is on screen after a scroll is code under test
reveal <name>               select that row *and* scroll it into view, as opening a bookmarked file
                            does. `select` deliberately does not scroll — clicking a row cannot,
                            since you had to see it to click it — so this is the only way a script
                            reaches ScrollIntoView, which in the tile view lands in
                            BringIndexIntoView, where the row has no element yet and the scrolling
                            has to come from the index alone
tree-scroll <px>            scrolls the sidebar's folder tree to a vertical offset, in pixels,
                            since nothing here synthesises mouse-wheel input
preview on|off              the active tab's preview pane, with its debounce and off-thread read
                            waited out (so assert after this, not straight after a `select`)
preview-mode auto|raw|hex   the pane's view override; sticky across selections, settled the same
                            way (`raw` is PreviewMode.Text — spelled the way the button is)
flat on|off                 the active tab's flat branch view — every file under this folder in one
                            list, no folder rows. Through the same command Ctrl+B
                            and the toolbar button use, so a script exercises one code path, and
                            settled. Unlike a search it survives navigation, which is what makes it
                            a mode; inside an open archive it lists the container's tree instead
flat-cap <n>                lowers how many rows one flat listing shows, so a fixture of a dozen
                            files can reach the truncated case and its banner. The one place a run
                            reaches past what a person could do, and it is here because the
                            alternative is fifty thousand real files

key <chord> [in list|text|readonly|other]
                            what pressing a chord does. **Not a key press** — a run posts no input.
                            It asks the same `KeymapRules::dispatch` the window's key handler asks,
                            with the focus posed (list unless said), and runs the command it names
                            through the same registry. Prints `KEY <chord> -> <id|nothing>`
run <command id>            run a command by its CommandCatalog id (tab.new, file.rename, select.all),
                            as choosing it in the palette does. Both this and `key` refuse to go
                            further than the availability check for a command that would open a
                            window, play media or close the app: they print `WOULD-RUN <id>`,
                            or `UNAVAILABLE <id> — <reason>` when the rules say no. Cut, copy and
                            paste do run, against the run's own clipboard (held in memory - the
                            person's is never read or written); `tools/ui/clipboard.bbs`
bind <command id> <chord>… | bind <command id> none
                            give a command exactly those shortcuts, taking each from whoever had it
unbind [command id]         back to the shipped shortcut; bare `unbind` resets them all
palette [open [seed]] | palette close
                            the command palette, in the window where it really is — so `shot`
                            photographs it. `palette open C:\x\` opens it on a path
palette type <text> | clear | down [n] | up [n] | tab | pin | shortcut | pick | rows
                            the box and the keys that work in it: `pick` is Enter (and reports
                            `WOULD-RUN` exactly as `run` does), `tab` carries on into a category or
                            folder, `shortcut` is "change its shortcut", `rows` prints the list
settings bars select menu-bar|title|row<n> | open <name> | up | pick <name> | add <item> |
              add-menu | remove <name> | nudge <name> up|down | name <text> | filter <text> |
              unplaced on|off | rows | choices
                            the Menus and toolbars page (`settings command-bars`), through its view
                            model: the middle list is one level of one bar, and `open` drills in
settings key select <id> | filter <text> | change | add | press <chord> | reassign | remove |
             reset | reset-all | find | cancel
                            the Keyboard page's recorder. `press` is the recorder being offered a
                            chord — the step after a key event has been turned into one

shot <name> [element]       PNG of the window, or of any x:Name'd element in it
settings <page> [dragging]  puts the settings page up in the main window, where the app shows it
                            (it is not a dialog), for a `shot`. Pages: general, appearance,
                            preview, search-index, history, new-items, columns, saved-searches,
                            workspaces,
                            context-menu, command-bars, keyboard.
                            The columns page shows the *saved default*, so put an arrangement in
                            front of it with `columns default` first; `dragging` adds a row's
                            insertion line (columns and context-menu), placed rather than dragged
                            since a run posts no mouse input; `new-command` (context-menu) presses
                            New command, so the editor it opens is in the shot
settings tick <label> on|off  sets a box on the open page by its label. Settings apply live on a
                            debounce, so `settle 800` before asserting what it changed
settings close              back to the folders, without the question an incomplete entry would ask
dialog <kind> [name]        PNG of a dialog: new-folder, new-file, rename, rename-advanced,
                            delete, delete-permanent, message, warning, properties,
                            theme-editor, disk-usage, duplicates, changes, undo-history, sync-preview,
                            sync-preview-running, search-syntax, saved-search, extract, compress,
                            archive-password, elevation,
                            columns, select-pattern, flat-large, checksum,
                            checksum-verify, compare-files, transfer, conflicts
                            (transfer is the queue window — the running job's items on top and
                            everything waiting underneath. It needs a `progress-demo` first, and
                            `progress-demo queued` to put jobs behind the running one.
                            conflicts needs a `conflict-plan` first: it shows what a real move
                            would clash with, each row carrying its own answer)
                            (checksum digests the selection and starts on load, unlike duplicates —
                            the user picked these exact files, so there is no whole-PC surprise to
                            guard against. checksum-verify is the same window pointed at a selected
                            checksum file, which `checksum-file` writes.
                            compare-files needs exactly two selected; whether it renders a line diff
                            or a hex dump is the file-compare plan's decision over the pair, so use
                            `write-text` pairs for the first and `write-binary` for the second)
                            (changes is the "What changed" window: with a run's default settings
                            it shows the recording-off banner — the state every fresh install
                            has — and after `changes-seed` it shows rows. `settings history` is
                            the page with its switch)
                            (both sync ones need a `compare` first, like `dialog duplicates`: they
                            show what that comparison found rather than starting one of their own.
                            sync-preview-running is the same window once Sync has been pressed —
                            the list read-only, and a bar and Cancel where the buttons were. Posed,
                            like `dialog transfer`, because it is a state that only exists while
                            something slow is happening)
                            (`columns` is the Add-column list, drawn in the popup layer of the window)
                            (new-folder/new-file/search-syntax need no selection; every other
                            kind uses one.
                            rename-advanced is the rename dialog with its options panel open —
                            the panel is opened by a click, and this never clicks)
                            (flat-large is the question a flat view asks before listing an enormous
                            folder, worded by the real `FlatViewRules` over a made-up dir_size_cache
                            row: a sandbox holds a dozen files, so it is the only way to see it)
state                       one JSON line of everything worth asserting on
session                     save the pane/tab arrangement the way closing the window does,
                            prune it the way a launch does, and reopen it in place — assert on
                            what came back with assert-panes / assert-tabs / assert-path
rows                        the row names, for when an assertion is about to fail
columns                     the list's columns as id:width, in order — a column header is not a
                            named element, so this is the way to check one
columns add|remove <id>     edit the active tab's columns through ColumnLayoutRules, the same
columns move <id> <index>   functions the header menu and a header drag go through
columns width <id> <px>
columns reset               back to the saved default
columns default             the header menu's "Set as default for new tabs" — what the settings
                            page reads, and the only way to seed it from a script
menu main <Menu> [name]     PNG of one of the menu bar's dropdowns, by its heading (`menu main File`)
                            — the rows a click would show, greyed and ticked as they would be. The
                            menu asserts below read it. Alt and F10 cannot be scripted: no input is posted
menu columns|files|background|tree|workspaces|saved-searches|undo|undo-range|redo [name]
                            PNG of a menu's items — the column header's, the file list's for the selection,
                            the file list's over empty space (the folder background), or the
                            folder tree's — for the tree's selected row, else the current folder.
                            They are rendered
                            detached, not opened: a menu run is never posted a click, and a native
                            popup could land on the user's screen. files/background/tree carry other programs' entries
                            (7-Zip, Git, TortoiseSVN) from a canned source, so they prove the
                            hosting and never what a real extension offers
tree-menu-run <item> on <folder>
                            runs one entry of the folder tree's menu for that folder, as a
                            right-click on its row and a click would: it acts on the folder, not
                            on the file list's selection (`tree-menu-run Bookmark on Pictures`)
assert-menu-item <text> | assert-no-menu-item <text>
                            whether the last `menu` offered an item containing the text, submenus
                            included — the check a screenshot cannot make
assert-menu-order <a> | <b> | …
                            the last `menu`'s visible top-level lines contain these in this order
                            (substrings; `-` is a separator), not necessarily adjacent
right-drop-menu <names> to <folder> [as <name>]
                            PNG of the menu a right-drag ends with, built for those sources over
                            that folder — so the wording and the greying are the real ones. Rendered
                            detached like `menu`, and nothing is dropped: a run cannot click, and a
                            script reaches the verbs themselves as move, copy and shortcut
probe <token> [element]     where a theme token's colour came out: the resolver, the app and
                            window resources, and the element's own Background/Foreground

assert-theme <id> | assert-following on|off
                            which theme resolved, and whether it is tracking Windows. Assert on the
                            id rather than on pixels — two dark themes differ by a few hex digits
assert-path <substring> | assert-status <substring> | assert-count <n>
assert-error [substring]     the warning banner above the list; bare = assert there is none
assert-row <name> | assert-no-row <name> | assert-selected <n>
assert-tabs <n> | assert-panes <n> | assert-flattened | assert-not-flattened
assert-realized under|over <n>
                            how many rows the list has actually built. The only way a script can
                            tell virtualized from merely fast: a thumbnail view of eight hundred
                            files must hold a screenful of containers, not eight hundred
assert-flat [on|off] | assert-not-flat
                            whether the tab is in a flat branch view; bare `assert-flat` means on.
                            Distinct from assert-flattened, which asks whether the rows come from
                            many folders — a search makes that true too, and the two differing is
                            the whole reason the tab carries both flags
assert-inside-archive | assert-not-inside-archive
assert-column <id> | assert-no-column <id>
assert-columns <id>, <id>, ...            the whole column order, injected ones included
assert-metadata <row> <canonical> <text>  what a shell-metadata cell actually reads (substring)
assert-header-menu columns|files          which menu a right-click past the last column opens
assert-can-undo | assert-cannot-undo | assert-exists <path> | assert-missing <path>
assert-transfer <substring> | assert-transfer-indeterminate
                            the running transfer's own strip — its headline and its figures
assert-queue <n> | assert-paused | assert-not-paused
                            how many jobs the queue holds, and whether it is held. `state` also
                            carries queueJobs, queueWaiting, queuePaused and queueKinds
assert-conflicts <n>        how many times a transfer has had to ask about a clash. Worth having
                            over checking what landed on disk: a paste onto a taken name used to
                            ask zero times and quietly number the newcomer, which looks exactly
                            like keep-both having been chosen
assert-conflict-rows <n>    how many rows the last question put up — after a `conflict-plan` or a
                            real transfer. The count is the proof a merged folder is not one row:
                            2 rows over a folder of a dozen files says the folder itself was
                            never asked about
assert-conflict-row <label> a row with that label was offered. Labels are relative to the drop
                            folder, so `photos/2024/a.jpg`, not `a.jpg`
assert-conflict-default <label>=<answer>
                            what a row started on before anybody answered — the one thing a
                            screenshot cannot prove, and how "an identical file defaults to skip"
                            is held down
assert-compare <substring>                the comparison banner's summary
assert-compare-row <row> <status>         a row's compare state, by the words its Status column
                                          shows. Read off the row, never off a screenshot: the
                                          tints are faint by necessity, so a pixel test would be
                                          asserting about the theme rather than about the verdict
assert-digest <name> <algorithm> <digest>
                            what the checksum window actually computed. Worth having over a
                            screenshot because the fixture is deterministic: a sandbox file holds
                            its own name repeated, so its digest is a constant
assert-verify <name> <state>
                            how one row of a verify run came out: Ok, Mismatch, Missing,
                            Unreadable, Refused, NotListed
assert-compare-files <substring>
                            the file-comparison window's verdict strip
assert-duplicate-groups <n> | assert-duplicate-selected <n>
assert-duplicate-row <name> | assert-no-duplicate-row <name>
assert-key <chord> <list|text|readonly|other> <command id|none>
                            which command a chord belongs to with focus there — the rule that used
                            to be "which control saw the key first"
assert-command <id> available|unavailable [reason]
                            whether a command may run right now, and that its reason mentions this
assert-gesture <id> <chord|none>
                            the shortcut a menu or tooltip prints for a command
assert-menu-gesture <item text> <chord|none>
                            what the last `menu` printed beside an item
assert-menu-bar <a> | <b> | …   the menu bar's headings, all of them, in order
assert-toolbar title|row<n> <item…> [+<n>]
                            what a toolbar is showing, exactly; `+2` for how many more are behind
                            its overflow button (none, when left out)
assert-toolbar-item title|row<n> <id> enabled|disabled|checked|unchecked|text|icon
assert-bars default|custom|<saved name>
assert-bar-rows <a> | <b> | …   the Menus and toolbars page's middle list, exactly (`-` a separator)
assert-bar-choice <name> | assert-no-bar-choice <name>
assert-tooltip <Name> <text>
assert-palette-open | assert-palette-closed
assert-palette-row <text> | assert-no-palette-row <text> | assert-palette-order <a> | <b> | …
assert-palette-selected <text> | assert-palette-hint <text>
                            the palette's list. Headings read `# Pinned`, categories read as their
                            token (`view:`), so both can be asserted on
assert-key-row <command name> <chord,chord|none|absent>
assert-key-recorder <text>  the Keyboard page's list and what its recorder strip is saying
assert-visible <Name> | assert-hidden <Name> | assert-not-launched
assert-elevation-offered | assert-no-elevation-offered
                            whether the run offered to retry something as administrator. The
                            negative form only means anything before the first offer — the
                            recording prompt accumulates over a run
assert-preview <kind>       image | document | text | hex | archive | font | media | loading | none

time <name> <verb> [args][; <verb> [args]…]
                            run the verb(s) under a stopwatch and print "BENCH <name> <ms>". Each
                            verb keeps its own settle, so the sample is "issued to quiet"; chain a
                            `select` with `settle-preview` to time a preview as a person sees it
repeat <n> <verb> [args][; <verb> [args]…]
                            the chain, n times — a soak. Owns the rest of its line, so not inside `time`
mem [name]                  "MEM {json}": the managed/heap bytes, working set, private
                            bytes, peak working set, items, realized rows, retained thumbnails
assert-mem managed|private|workingset under <mb> [since <name>]
                            a ceiling, or with `since` a budget on growth over an earlier `mem <name>`
                            — the leak check: `mem before` · `repeat 25 …` · `assert-mem managed under 8 since before`
settle-thumbnails [ms]      wait until no icon or thumbnail request is queued at the shell gates, then
                            settle — Settle deliberately does not wait for pictures; a tile scenario
                            says so when it needs to (prints "SETTLED-THUMBS <ms>")
settle-preview              wait for the preview pane's debounced load to start and finish
bench-fixture <files> <dirs> [dir]
                            that many empty files, straight in (dirs=0) or round-robin over that many
                            subfolders; named by index so a search has a known hit count. For the
                            bench-*.bbs scenarios; `many-fixture` is still the tile one

echo <text> | sleep <ms> | settle [ms]      '#' at the start of a line is a comment
```

Options: `--script <path>` · `-c "<cmds; ...>"` · `--out <dir>` · `--sandbox <dir>` · `--state-dir <dir>` ·
`--keep-state` · `--allow-outside` · `--size WxH` · `--theme <id>` · `--start <path>` · `--index`
(accepted and ignored: the native harness has no index service yet) · `--index-declined` (poses the
declined elevation prompt the index banner reacts to) · `--timeout <sec>` · `--busy-timeout <ms>` ·
`--keep-going` · `--verbose` · `--bench-out <file>` (append `time`/`mem` samples as JSON lines, plus the
session's own startup as `startup.hosted`; what `bb-bench ui` reads). `--help` prints them all.

Element names, which `shot` and the `assert-visible`/`assert-hidden` verbs take: window-level are
`FolderTree`, `GlobalSearchBox`, `ThumbSlider`, `PaneHostSite`, `TitleBarRoot`, `ToolbarRows`,
`CompareBanner`; per tab (resolved against the *active* tab) are `FileListView`, `SearchBox`,
`PathBox`, `Breadcrumb`, `DetailsView`, `PreviewPane`, `SidePanePreview`, `SidePaneMetadata`; per pane,
`TabHost`. They are widget names in the C++ tree, kept identical to the old XAML names so the scripts
did not change.

## Blocked on purpose

- **Nothing is launched.** The core is given a launcher that refuses, because opening a file starts
  whatever program owns it and *that* window lands on the user's desktop. So `enter` on a file is
  refused, and "Open in Terminal", custom commands and the portable-device handler all report rather
  than run. `assert-not-launched` proves a gesture did not even try. The folder-handler registration is
  refused the same way.
- **The clipboard is not touched.** There is one per session and the user is using it. `move` and
  `copy` go through the same planner and executor paste and drag-and-drop go through, without it.
- **Writing outside the sandbox is refused.** This app deletes and moves real files, and the harness
  drives the real executors. `--allow-outside` is the deliberate way past, for when you mean it.
- **No UAC prompt is ever raised.** A file operation Windows refuses can be retried with an
  administrator token, and a prompt takes the *secure desktop* - the one thing parking the window
  offscreen cannot work around. The run gets a refusing elevation launcher, and the session asserts
  after creating the app that every such hazard seam is faked rather than trusting the configuration.
  Everything above the launcher is real, so the discriminator, the rules and the merge are genuinely
  exercised; only the process and the token are missing. `dialog elevation` poses the consent window.
- **A modal the shell raises is recorded, not shown.** The notice, confirm and prompt seams are how the
  shell says something that must be acknowledged - so far "comparing needs two panes", the
  are-you-sure questions, archive passwords. A real one is a dialog a run could never dismiss; a run gets
  recording fakes, which is also what makes the wording testable (`compare-refused`).
- **No media is ever played.** The preview pane stops at a poster frame until someone presses play, and
  no script presses it. That keeps a run silent on a machine someone is using. Assert media through
  `state`, never a screenshot.
- **The MFT indexer does not run.** It reads every NTFS volume's master file table - minutes of disk on
  a machine someone is using - and needs administrator rights the harness does not request.
  `--index` is accepted and ignored for now; `gsearch` says so rather than reporting an empty result as a
  finding, and `--index-declined` poses the "user said no" state for the banner.

## Recipes

**Look at a change you just made**

```powershell
& $harness -c "tree .; refresh; shot after-change" --out $env:TEMP\look-1
```

**A dialog, in both themes**

```powershell
& $harness -c "tree .; refresh; select notes.txt, report.md; dialog rename dark; theme light-plus; dialog rename light" --out $env:TEMP\look-2
```

**Prove a move and its undo really happened on disk**

```powershell
& $harness -c "tree .; refresh; select photo.jpg; move to Documents; assert-exists Documents/photo.jpg; undo; assert-missing Documents/photo.jpg; assert-exists photo.jpg" --out $env:TEMP\look-3
```

## What to trust

- Captures are a software re-render of the widget tree into an offscreen Direct2D bitmap on WARP, not a
  screen grab, so being offscreen costs nothing and a fullscreen game in front of the window changes
  nothing. A `shot` that comes back one flat colour fails rather than lying to you.
- **Do not diff UI screenshots pixel for pixel** - text rasterisation varies with font version and
  DPI. Assert through `state`, `assert-status`, `assert-count`, `assert-row`.
- **The fixture is deterministic** - same names, same byte counts, one fixed modified date - so two
  captures of the same script differ only where the change under test made them differ. Anything
  outside it is not: the folder tree shows the real machine's drives. Thumbnails in a run are generated
  (a deterministic picture per row), never the shell's.
- Each run gets a scratch data directory (`--state-dir`, `BERTBROWSER_DATA_DIR` inside the process), so
  it cannot read or corrupt the user's real search index, settings or themes, and it is deleted
  afterwards. Use `--keep-state` only when you mean to test something across two runs.
- **A run reporting more than one foreground correction means something new is activating the
  window** - worth investigating rather than ignoring. The `ForegroundGuard` hands the foreground
  straight back; the count is in `state`.
- `sandbox` paths under `%TEMP%` sit inside `AppData`, which is hidden - so the folder tree cannot
  reveal down to them with "Show hidden items" off. Use `--sandbox` somewhere visible (with
  `--allow-outside`) if a capture is about the tree.
