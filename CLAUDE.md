# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

bertbrowser is a Windows-only WPF file browser (net10.0-windows) with global MFT-backed search and cached recursive directory sizes, backed by a local SQLite database.

## Commands

```powershell
dotnet build bertbrowser.sln          # build everything
dotnet test bertbrowser.sln           # run all tests (xUnit, Core only)
dotnet test tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests"   # one test class
dotnet test tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests.MethodName"  # one test
dotnet run --project src/BertBrowser.App   # launch the app (optional arg: start directory)
```

`Directory.Build.props` sets `TreatWarningsAsErrors` and `Nullable` for all projects — any warning fails the build.

If a build fails with MSB3021/MSB3026 because a running BertBrowser instance locks `bin\Debug`, just kill it (`Get-Process BertBrowser | Stop-Process -Force`) and rebuild — no need to ask or build to a scratch directory.

## Structure

- `src/BertBrowser.Core` — plain net10.0, no UI dependencies: SQLite persistence, path canonicalization, search/size services. This is the only project with tests; keep anything testable here rather than in the App.
- `src/BertBrowser.App` — WPF shell. MVVM via CommunityToolkit.Mvvm source generators (`[ObservableProperty]`, `[RelayCommand]` on `partial` classes), DI via `Microsoft.Extensions.DependencyInjection`. The composition root is `App.xaml.cs` (`App.Services`): register new services/repositories there.
- `tests/BertBrowser.Core.Tests` — xUnit; tests create real SQLite databases and directory trees under `%TEMP%`.

## Key architecture

### Path keys (critical invariant)

All database path storage goes through `BertBrowser.Core.Paths.PathKey`:

- `Canonicalize()` produces the DB key: fully qualified, `\` separators, no trailing separator (except drive roots like `C:\`), **uppercased invariantly**. Case folding happens in C# because SQLite's NOCASE collation only folds ASCII; DB columns compare with plain BINARY collation.
- `NormalizeDisplay()` is the same normalization but casing-preserving, stored separately for display (`file.display_path`).
- `PrefixBounds(dir)` returns a half-open `[dir+'\', dir+']')` range so recursive "everything under this directory" queries are pure index range scans (`]` is the character after `\`). Use this instead of `LIKE` for subtree queries.

Any new table keyed by path must store `PathKey.Canonicalize()` output and query subtrees via `PrefixBounds`.

### Database

`Db` (Core/Data) is the connection factory and migration runner. Connections open in WAL mode with foreign keys on. Migrations are embedded resources at `Data/Migrations/NNN_*.sql`, applied in order and tracked via `PRAGMA user_version`. To change the schema, add a new numbered `.sql` file — the csproj glob picks it up; never edit an existing migration. The live DB is `%USERPROFILE%\.bertbrowser\bertbrowser.db` (settings: `settings.json` in the same folder; paths come from `AppPaths` in the App project). Data must never live in `%LOCALAPPDATA%\BertBrowser` — that is the Velopack install directory, deleted on uninstall.

Repositories (`DirSizeRepository`, `FsIndexRepository`, `BookmarkRepository`) are synchronous ADO.NET; the services above them (`BookmarkService`, `SearchService`, …) are the async facades that keep ViewModels off the SQLite calls. Follow that layering for new data access.

### Directory sizes

**Nothing walks the filesystem to size a folder.** Every number comes from `dir_size_cache`, which
`MftDirectorySizeBuilder` fills for *every* directory on a volume as a side effect of the MFT index
pass (see `docs/search-indexing.md`) — so sizes appear instantly and the app never has an on-demand
recursive scan competing with the indexer. Readers (`FileListViewModel.HydrateDirSizesAsync`,
`FolderTreeViewModel`, `PropertiesViewModel`) do a batched `DirSizeRepository.GetMany` and nothing
else; a missing row means unknown and must render as blank or "not indexed", **never as zero** —
that is what a non-NTFS volume or a still-indexing one looks like. The earlier on-demand
`DirectorySizeService` (a .NET post-order DFS behind a "Compute size" menu item) is gone; do not
reintroduce a scan-on-demand path.

### Transfers (move / copy / drag-and-drop)

`Core/Services/Transfer` is the only code that relocates user data; `FileTransferService` (paste) is
a thin facade over it, so there is exactly one implementation to audit. It is split deliberately:

- **`TransferPlanner`** decides, touching nothing. It refuses a folder dropped onto itself or into
  its own subtree, checking containment on both the literal and the link-resolved paths so a
  junction can't smuggle the destination inside the tree being moved; drops sources nested under
  another source (they travel with the ancestor); refuses drive roots; and flags name conflicts.
  It talks to disk only through **`ITransferProbe`**, so those rules are unit-tested against link
  layouts that need privileges to create for real.
- **`TransferExecutor`** writes, and **re-applies every planner rule against live disk state first**
  — a plan is built while the drag hovers and executed on drop. Its invariants: nothing is deleted
  to make room (`Replace` moves the displaced entry to a hidden `.bertbrowser-replaced-*` staging
  folder, and puts it straight back if the transfer then fails); a cross-volume directory move
  copies, verifies file count and total bytes, and only then deletes the source; a tree containing
  junctions is refused across volumes rather than copied without them; one item's failure never
  affects the others. `Directory.Move` falls back to copy-then-delete **only** on
  `ERROR_NOT_SAME_DEVICE` — any other `IOException` is a real failure and must surface.

Undo (Ctrl+Z, one level) reverses the last move and restores anything a Replace displaced.
`ShellViewModel.RetireUndoable` is what finally commits a replacement, so staged data outlives the
undo record by exactly one operation. Copy has no Replace and no undo: it is defined as purely
additive. The undo slot is **shared with rename** — one level, whichever happened last — so both
sides call `RetireUndoable` before claiming it, and `IsTransferring` gates both.

Tests are the point of this design — `TransferPlannerTests` (rules, fake filesystem),
`TransferExecutorTests` (real files, contents asserted), and `TransferRoundTripTests` (property
tests: the multiset of file contents under the root is invariant under any move, and undo restores
the tree byte-for-byte). Two meta-tests assert those invariant checks can actually detect a lost or
moved file. If you change this code, mutate a rule and confirm a test goes red.

### Rename

`Core/Services/Rename` is split the same way and for the same reason. F2 or the context menu opens
`Views/RenameDialog` over the selection; one item takes the typed text as its whole name, several
are **numbered** — "Holiday" over three photos gives `Holiday 1.jpg`, `Holiday 2.png`,
`Holiday 3.jpg`, each keeping its own extension (a folder has none, so `My.Project` is not treated
as having one). The count follows the order the *list* shows, not the order rows were clicked.

- **`RenamePattern`** is the naming and legal-name rule, pure and shared: the dialog previews every
  keystroke with the same function the rename obeys, so a preview cannot drift from the result.
- **`RenamePlanner`** decides, touching nothing, through **`IRenameProbe`**. Nothing may land on a
  taken name — but a name held by *another selected item* is fair game, since that item is about to
  vacate it, which is what makes rotating or shifting a numbered set work. Only if it really is
  leaving: a second pass drops anyone aiming at an item whose own rename was refused, and repeats,
  because one refusal can doom another.
- **`RenameExecutor`** writes, re-checking each name against live disk state first (the plan is
  built while the dialog is open). Nothing is ever overwritten — both moves are the non-replacing
  overloads. An item whose current name *another* item wants is moved to a `.bertbrowser-rename-*`
  name first, so the second pass only writes into empty space; a failure there puts it back, and if
  even that fails the staged path is named in the error rather than left silently. A case-only
  rename needs none of this — .NET handles it directly.

A rename is its own inverse, so **undo is the same execution with every path swapped**
(`RenameExecutor.Undo` runs `UndoPlan` back through `Execute`) — which is what makes undoing a
rotation, or a batch that shifted a numbered set along, go through exactly the staging the forward
direction did. It shares the one-level undo slot with transfers (see above), and an old name that
has since been taken is reported rather than overwritten.

A selected folder and something inside it are both renameable in a flattened search result, so the
planner refuses the inner one (`InsideARenamedFolder`): renaming the folder first would move the
other item's path out from under it.

The dialog refuses the whole rename while any item is rejected, rather than half-doing a batch, and
reports per-item failures afterwards in a `MessageDialog`. `ShellViewModel.RenameAsync` fans out
like a transfer does, plus `FollowRenamedFoldersAsync`: a tab sitting inside a folder that was just
renamed is re-pointed at the new path instead of being left on a name that no longer exists.

Tests: `RenamePatternTests` (naming and validation), `RenamePlannerTests` (rules, fake filesystem),
`RenameExecutorTests` (real files, contents asserted, undo included, with a meta-test that the
"nothing stranded" check can fail). Mutate a rule and confirm a test goes red.

### Delete

`Core/Services/Delete` is split the same way again, and rests on one idea: **an ordinary delete does
not erase anything.** Shift+Delete is the other path: erase in place, no holding, no undo, and the
confirmation says so in a red banner rather than in a footnote.

An ordinary delete goes to the **Windows Recycle Bin**, so deleted items are where every other app
on the machine puts them and stay there until the user empties it. Where a volume has no working bin
— a network share, removable media with it turned off — the item falls back to this app's own hidden
holding folder, a *move* rather than a copy, so a hundred gigabytes costs what one file costs. Both
routes are undoable; they differ in how long the data outlives the undo record, which is why the
confirmation says which one an item is taking.

`DeleteMode` is what the user asked for (`Recycle` / `Staged` / `Permanent`); `DeleteDisposition` is
what will happen to one particular item. **The planner decides the disposition, not the executor**,
because that is what lets the confirmation describe what is really about to happen.

- **`DeletePlanner`** decides, touching nothing, through **`IDeleteProbe`**. It refuses drive roots
  and a small set of **`ProtectedLocations`** (Windows, Program Files, ProgramData, the profile
  root) — *the folders themselves, never their contents*, because this app runs elevated for its MFT
  index and the usual "Windows will stop you" backstop is not there. An item selected together with
  a folder above it is dropped as a **benign** no-op (`InsideADeletedFolder`): it is being deleted,
  just as part of its ancestor. The protected set is constructor-injected so the rule is testable
  without depending on where Windows happens to be installed.
- **`DeleteSurveyor`** measures — files, folders, bytes — so the confirmation can say what is going
  rather than how many rows were selected. It skips reparse points instead of following them (a
  junction is the one entry deleting it removes) and marks a result `Incomplete` rather than
  throwing, so the totals are honest about being a floor. It is cancellable and nothing depends on
  it: a delete whose survey never finished deletes exactly the same items.
- **`DeleteExecutor`** writes, re-applying the planner's rules against live disk state first —
  including the disposition, since an item can lose its Recycle Bin between plan and execution. With
  no bin to hand it to the answer is the holding folder, **never an erase**. The holding folder is
  `<volume root>\.bertbrowser-trash\delete-<id>`, hidden — at the volume root so a move is a rename,
  one place per disk, and sweepable later. `Directory.Move` reporting `ERROR_NOT_SAME_DEVICE` (a
  mount point part-way down the path) is the one case that falls back to a `.bertbrowser-deleted-*`
  folder beside the item, which cannot be on another volume.
- **`Interop/ShellRecycleBin`** (App) is the bin itself, over `IFileOperation` on an STA thread with
  a deadline, and it implements both `IRecycleBin` and `IRecycleProbe` from one object so the
  planner and the executor cannot disagree about what has a bin. `IFileOperation` rather than
  `SHFileOperation` for one decisive reason: its progress sink's `PostDeleteItem` hands back
  `psiNewlyCreated`, the item's `$R` path *inside* the bin — which is what undo restores from, and
  is what keeps it correct when the same path has been deleted twice. A null there means the shell
  erased the item rather than holding it, which is not a failure but leaves nothing to undo, so
  `DeleteOutcome.CanUndo` asks each item rather than trusting the mode.

  **The flags are the dangerous part.** `FOF_ALLOWUNDO` with `FOF_NOCONFIRMATION` means "if it
  cannot be recycled, erase it without asking". Two things stand against that: the planner routes
  binless volumes to staging before the shell ever sees them, and **`FOF_WANTNUKEWARNING` stays
  set**, which overrides `FOF_NOCONFIRMATION` for exactly the case pre-flight cannot predict (an
  item over the bin's quota). That is the only OS-drawn dialog this app permits, and it is
  deliberate. `FOFX_EARLYFAILURE` is deliberately *absent* — it would abandon the rest of the batch
  on the first error, and everything else here holds that one item's failure must not cost the
  others.

  Restore goes through the shell's own **canonical `undelete` verb** (never the localised menu
  text). `InvokeVerb` returns nothing at all, so success is established by watching for the original
  path to reappear on a short deadline; a timeout is reported as a failure naming the `$R` path
  rather than being assumed to have worked.

Three things here are load-bearing:

- **`CommitStaging` is what actually erases**, and `ShellViewModel.RetireUndoable` is the only
  caller — so held data outlives the undo record by exactly one operation, the same contract a
  Replace's staging has. `MainWindow.Closing` retires too, or a session would end with its last
  delete still on disk. The undo slot is now three-way (move / rename / delete), one level, whichever
  happened last. **A recycled item has no staging lifecycle at all** — it contributes no holding
  folder, so commit no-ops over it and the data simply stays in the bin. That is the structural gain
  of the Recycle Bin being the default, and it is why nothing in this list needed changing for it.
- **`IsStagingDirectory` is the guard on every recursive delete in this file.** A path only qualifies
  if it is named the way this class names holding folders. Mutate it to return true and
  `CommittingRefusesAnythingThatIsNotAHoldingFolder` deletes a real folder and goes red — that is
  the check standing between a mangled outcome and `Directory.Delete(recursive: true)` on anything.
- **A crash leaves holding folders behind**, so `PurgeAbandonedStaging` sweeps every ready volume at
  startup — but only batches over a day old, so a second copy of the app running right now keeps its
  pending undo.

`Views/DeleteDialog` is the confirmation: every item with its icon, the folder it is leaving, and
what it amounts to, filling in as the survey walks. Cancel is the **default** button — nothing
destructive should be one stray Enter away — and the planner's refusals are shown in the dialog
rather than turning into a silently shorter delete. Delete/Shift+Delete are on the file list's own
`KeyDown` beside F2; the folder tree's context menu offers the reversible delete only, since a drive
root is one careless click away in that list. `ShellViewModel.DeleteAsync` fans out like a transfer
does, plus `LeaveDeletedFoldersAsync`: a tab sitting inside a folder that was just deleted is moved
up to where that folder was, rather than left on a path that no longer exists.

One thing the Recycle Bin quietly breaks unless you look for it: **`$Recycle.Bin` has to be excluded
from search**, or a recycled file keeps turning up in global MFT results under a name that is not
even the one that was deleted (`C:\$Recycle.Bin\S-1-5-21-…\$RAB1234.txt`). `DeleteExecutor.IsHeldPath`
is the one predicate `SearchService` asks, and it now covers the bin alongside
`.bertbrowser-trash`. `ProtectedLocations.IsInsideRecycleBin` refuses deleting anything *in* the bin
— unlike the exact-match protected set, the contents are covered too, and deliberately: those `$R`
files are what Ctrl+Z restores from.

Tests: `DeletePlannerTests` (rules and Recycle Bin routing, fake filesystem and a fake probe),
`DeleteExecutorTests` (real files, restored trees compared file-by-file against a pre-delete
snapshot, with meta-tests that the comparison notices a missing file and changed contents; the bin
is a `FakeRecycleBin` that really moves files, so a **mixed batch** — half recycled, half staged —
and its undo are asserted on contents), `DeleteSurveyorTests` (counts and bytes). The executor takes
a `stagingRoot` purely so tests do not create folders at the root of a real disk. Mutate a rule and
confirm a test goes red — `DispositionFor` returning `Recycle` unconditionally, or the executor
erasing when there is no bin, both go red.

### Launching other programs

**Nothing in this app calls `Process.Start` any more** — there is exactly one, inside
`ProcessLauncher`, and a `git grep` finding a second one is a bug. The reason is that this process
holds an administrator token for the MFT, and a child started directly inherits it *with no prompt*:
double-clicking a downloaded `.exe` would run it as administrator. The split is three ways:

- **`Interop/ShellLauncher`** is the mechanism, and the trick is that it does not launch anything.
  `explorer.exe` already runs at the user's own integrity level and publishes its automation object
  as `ShellWindows`; reaching that gets an `IShellDispatch2` **living in explorer's process**, and
  asking *it* to `ShellExecute` makes explorer the parent, so the child gets explorer's token. This
  is also what makes elevation honest: `runas` from *this* process would elevate silently — we
  already hold the token — while from medium-integrity explorer it is a real request and Windows
  prompts.
- **`Services/ProcessLauncher`** (`IProcessLauncher`, DI) is the policy, and the policy is one
  sentence: **nothing starts elevated unless the user chose it.** When the shell can't be reached it
  refuses and asks rather than falling back to the elevated launch that was the original problem.
- **`Core/Services/ExecutablePath`** resolves a program name to a full path *before* the handover.

Four things here are load-bearing:

- **The COM interfaces are hand-declared and every method ahead of the one we call must be present**,
  in vtable order, or the call lands on the wrong slot — which is an access violation, not a
  catchable exception. `ShellWindows` has eight members before `FindWindowSW`; `IShellBrowser` and
  `IShellView` each derive from `IOleWindow` and have twelve before the one we use.
- **Resolving first is not a nicety.** `ShellExecute` returns `void` — no handle, no exit code, no
  "not found". "Open in Terminal" used to discover that Windows Terminal was missing by letting
  `Process.Start` *throw* and falling through to PowerShell in the `catch`; with the throw gone, a
  null resolve is that signal. It also decides *what* runs here rather than leaving a bare `code` for
  an elevated process to look up. `ExecutablePathTests` covers it — including that a path which
  isn't fully qualified is never probed, since that would resolve against the working directory.
- **A timeout is its own outcome** (`ShellLaunchResult.Unresponsive`), distinct from "unavailable",
  and nothing may be retried after it: the shell may still be mid-launch, so offering an alternative
  would start the thing twice.
- **`AllowSetForegroundWindow`** hands foreground rights to explorer before the call. Without it the
  launched window — and the UAC prompt, which explorer owns rather than us — opens *behind*
  BertBrowser.

Elevation is offered three ways: the file list's **Run as administrator** item (one file only),
Ctrl+Shift+double-click / Ctrl+Shift+Enter, and a per-command checkbox on custom commands. The
plain-Enter arm of `FileList_KeyDown` needs its modifier guard or it swallows Ctrl+Shift+Enter
first, and the double-click handler resolves the row **under the cursor** rather than
`SelectedItem`, because Ctrl+Shift has already told an `Extended` `ListView` to range-extend. The
folder tree deliberately has no elevated entry — a drive root is one careless click away there.

The shell chain can't be unit-tested; it was verified with a scratch harness that compiles
`ShellLauncher.cs` directly and compares the integrity level of a child launched each way
(`Process.Start` → High, shell route → Medium). Rebuild that harness rather than trusting a fake.

### Startup, the command line, and single instance

One copy runs per user. A second launch parses its arguments, hands them to the first over a named
pipe, and **returns from `Main` without ever calling `app.Run()`** — no WPF, no DI, no database. This
is not only convenience: two copies each run their own MFT indexer against the same SQLite file, and
`DeleteExecutor.PurgeAbandonedStaging` only skips batches under a day old *because* a second copy
might be holding a pending undo. One instance is what makes that assumption sound.

- **`Core/Cli/CommandLine`** parses, and is **pure** — it never touches the filesystem, so "does
  this exist?" stays with the caller and every rule is testable. It understands several paths,
  `--new-tab`/`-t`, `--new-pane`/`-p` (this app splits panes rather than opening second windows, so
  there is deliberately no `--new-window`), and Explorer's `/select,<path>` in both the one-token and
  two-token spellings. An unrecognised option becomes an **error, never a path** — a mistyped flag
  opening the profile folder is worse than a message. It also repairs the mangling everyone hits
  once: `"C:\Dir\"` reaches argv as `C:\Dir"`, because the backslash escaped the closing quote.
- **`Core/Cli/NavigationRequest`** is the wire format *and* `IsAcceptablePath`, the single rule
  deciding whether a path may be acted on — used by the command line and the pipe alike, so there is
  one place to audit rather than two that drift. It is `ThemeId.IsSafe`'s counterpart for IPC:
  absolute local or UNC only, no device paths, no wildcards, no control characters (which is what
  makes tab a safe field separator). Mutate it to accept and 19 theories go red.
- **`Services/SingleInstance`** owns the mutex and the pipe. Claimed **after**
  `VelopackApp.Build().Run()`, whose hooks exit the process and must not be gated behind an instance
  check. Both peers are elevated processes of the same user, so the pipe needs no mandatory-label
  work — a DACL admitting only the current user's SID is right, and the client's identity is
  re-checked after connect anyway. **A failed hand-off falls through and starts normally**: the first
  copy may be mid-shutdown, and starting is better than exiting having done nothing.

The protocol is deliberately one verb — *navigate to this path*. Nothing on the wire can become a
launch, a file that gets written, or anything but a directory listing.

`ShellViewModel.OpenRequestAsync` carries a request out, resolving each target against disk (a
target naming a file opens its folder and highlights it, which is the only useful reading of "open
this file" for a file browser). Highlighting goes through `DirectoryTabViewModel.PendingSelection`,
a one-shot the **view** consumes once `FileListViewModel.Items` has been replaced — the selection
lives in the `ListView`, so nothing else can apply it, and not before the rows exist. It is applied
*before* `FocusFileList`, since selecting scrolls.

### Theming

The app is themed edge to edge, VS Code Dark+ by default. **No colour literal belongs in XAML or
C# any more** — everything goes through a named token.

- **`BertBrowser.Core.Theming`** holds the whole model, because it is the part worth testing:
  `ThemeToken` (the ~109 token keys plus editor metadata), `ThemeColor` (ARGB struct, hex parsing,
  HSV, WCAG luminance), `ThemeDefinition`/`ThemeJson` (the on-disk format for user themes),
  `ThemeCatalog` (the built-ins **as C# data**, not files, so every shipped colour is visible
  to xUnit), and `ThemeResolver`. Resolution **never throws**: an unknown token, a bad hex literal,
  a missing or cyclic base theme each become a `ThemeIssue` and the caller still gets a complete
  theme. Dark+ and Light+ are roots defining every token; every other built-in is a sparse sheet
  over one of them (the dark ones over Dark+, Catppuccin Latte / Rosé Pine Dawn / Solarized Light
  over Light+), resolved through the same inheritance path a user's own theme uses — so that path is
  exercised by everything we ship. A new built-in is a definition plus an entry in `BuiltIns`, whose
  order is the picker's order. Two things bite when authoring one: `ThemeCatalogTests` runs the whole
  contrast suite over it, so a palette's published colours usually need darkening before they clear
  AA against white text (Nord's frost, Everforest's green and Catppuccin's mauve all did) — and an
  eight-digit literal is **`#AARRGGBB`**, so a value copied from a VS Code theme, which writes
  `#RRGGBBAA`, silently parses as a different colour rather than failing. Ayu Mirage and Cobalt2 are
  the exceptions that prove the accent tokens are not assumed dark: both keep a light accent and
  override `Accent.Foreground`/`Text.OnAccent` to the base colour instead.
- **`BertBrowser.App.Theming`** materialises it. `ThemeTokenDictionary` is a `ResourceDictionary`
  holding one brush per token; `ThemeService` resolves a definition and recolours them.

Four things here are load-bearing and easy to undo by accident:

- **Brushes are recoloured in place, never replaced**, which is why `{StaticResource Theme.X.Y}`
  works everywhere and nothing needs `DynamicResource` or rebinding. Consequently a token brush
  **must never be frozen** — and WPF will try: a `ResourceDictionary` seals its `Freezable` values
  when the `Application` adopts it. `ThemeTokenDictionary.ThemeBrush` sidesteps that by binding the
  brush's `Color` to a holder, because a freezable carrying an expression reports
  `CanFreeze == false`. Assigning `SolidColorBrush.Color` directly does not survive startup.
- **`StaticResource` reaches a dictionary's own merged children, not its siblings.** A control
  dictionary that merely sits next to the tokens in some parent's list resolves them to
  `UnsetValue` and fails at load. So every file that names a `Theme.*` key merges `Theme/Tokens.xaml`
  itself (the `Controls/*.xaml` do it via `Controls/Primitives.xaml`), and the instances share one
  static set of brushes so it is still a single palette.
- **An explicit `Style` replaces the implicit one.** Keyed button styles say
  `BasedOn="{StaticResource {x:Type Button}}"` or they silently fall back to the Aero template.
  Likewise there is deliberately **no `Foreground` setter on the implicit `TextBlock` style**: a
  setter beats inheritance, which would repaint selected rows, the status bar and highlighted menu
  items back to the body colour.
- **`Colors` are value types and cannot be recoloured**, so the few consumers that need one (the
  pinned-row `DropShadowEffect`) use `{DynamicResource Theme.X.Y.Value}`.

Things that look done but aren't unless you check: `GridViewColumnHeader` needs `PART_HeaderGripper`
(or column resize breaks silently) and a blank template for `Role=Padding` (or a classic strip shows
after the last column); menu separators resolve through `MenuItem.SeparatorStyleKey`, not an
implicit `Separator` style; `TextBox` needs explicit `CaretBrush`/`SelectionBrush`.
A retemplated `ComboBox` needs `ItemTemplate`, **not** `DisplayMemberPath`: the closed box renders
`SelectionBoxItemTemplate`, which WPF derives from `ItemTemplate` alone — the `DisplayMemberPath`
fallback lives in the stock presenter and disappears with the stock template, leaving the box showing
the item's `ToString()`. The theme pickers share `ThemeNameTemplate` from `Resources/Styles.xaml`.
`ThemeSystemColors` overrides the `SystemColors.*Key` entries as a net for anything not retemplated.
`MessageBox` is OS-drawn and cannot be themed — use `Views/MessageDialog` instead.

`Views/ThemedWindow` gives all five windows a `WindowChrome` title bar. It is a `Window` subclass,
not just a style, because three things need the HWND: clamping a maximised window to the monitor
work area (`WM_GETMINMAXINFO` — a fixed margin is DPI-wrong), answering `WM_NCHITTEST` with
`HTMAXBUTTON` so Windows 11 offers snap layouts, and `DwmSetWindowAttribute` for the frame DWM still
draws (its colours are `COLORREF`, i.e. BGR). `MainWindow` puts its navigation toolbar in the caption
strip via `TitleBarContent`; anything interactive up there needs
`WindowChrome.IsHitTestVisibleInChrome`.

**The `WindowChrome` from that style is sealed and shared.** It arrives from a `Setter`, so assigning
to any of its properties throws "in a read-only state" — and since a `Setter` value is one instance
handed to every window, an assignment that *did* work would change all of them. `OnApplyTemplate`
therefore **clones** it to drop the resize border on a `NoResize` dialog. This failure mode is nasty:
it throws from `Window.Show`, so the window never opens, and a caller that discarded the `Task`
(`_ = SomethingAsync()`) shows nothing at all — which is what "the menu item does nothing" looks
like.

**An implicit style keys on the element's exact runtime type and never walks the base chain.** No
window here *is* a `ThemedWindow` — they are all subclasses of one — so `Style TargetType="{x:Type
v:ThemedWindow}"` reaches nothing by itself, and the failure is silent: every window falls back to
the stock `Window` template, which means a native caption and `TitleBarContent` quietly dropped. The
`ThemedWindow` constructor's `SetResourceReference(StyleProperty, typeof(ThemedWindow))` is what
makes the subclasses pick the style up, and it is load-bearing. Testing this needs a *subclass* — a
bare `new ThemedWindow()` matches its own implicit style and passes either way.

Settings has an **Appearance** section that applies and persists immediately — deliberately outside
the dialog's copy-then-commit contract, since a theme you cannot see until you press Save is not a
theme you can choose. "Customise colours…" closes Settings and opens the **modeless**
`ThemeEditorWindow`, which must not be modal or it would cover the file list you are judging the
colours against. User themes live in `%USERPROFILE%\.bertbrowser\themes\*.json`; a theme that is
temporarily missing falls back for the session **without rewriting `AppSettings.ThemeId`**.
`ThemeId` is nullable: null means "never chosen", which is what lets a first launch honour a Windows
high-contrast setting.

**A theme's id is also its filename, and an imported theme's id is untrusted input.** The app runs
elevated, and `Path.Combine` discards its first argument entirely when the second is rooted — so an
imported `"id": "C:\\Windows\\Temp\\evil"` would have written there rather than into the themes
folder. `BertBrowser.Core.Theming.ThemeId` (not `AppSettings.ThemeId`, which is unrelated) is the
single rule: `IsSafe` decides whether a string may become a filename — one path segment, no
traversal, no DOS device name, nothing Windows silently rewrites like a trailing dot or space — and
`Unique` manufactures a safe id from a display name, falling back rather than ever returning
something `IsSafe` rejects. `UserThemeStore.PathFor` throws on an unsafe id instead of resolving it,
`TrySave`/`TryDelete`/`Load` check first so untrusted input is reported rather than thrown on, and
`ThemeService.TryImport` keeps an imported id **only** if `IsSafe` passes. `ThemeIdTests` covers it;
mutate `IsSafe` to accept and the traversal theories go red.

`ThemeCatalogTests` is the guard worth keeping green — it asserts every built-in defines every
token, parses, and clears WCAG contrast for body text, selected rows, the status bar and menu
highlights. Mutate a colour toward its background and it goes red.

### Tabs and panes

Several directories are open at once, so **nothing may reach for "the" current directory**. The
hierarchy is:

- **`DirectoryTabViewModel`** — one browsable directory, and the unit everything else repeats.
  Owns `CurrentPath`, the back/forward stacks, a `CancellationTokenSource` per navigation, the
  search box state (text, scope, 200 ms debounce), `StatusText`, `SelectionSummary`, its
  `FileListViewModel`, and the navigation/open/compute-size commands. It has no reference to the
  shell: `IncludeHidden` reads `AppSettings.ShowHiddenItems` directly, and `SelectedItems` is
  mirrored out of the `ListView` so nothing has to reach into a view to find the selection. Tabs are
  closable, so unlike the shell they `Dispose()` — cancel in flight work and unsubscribe.
- **`PaneViewModel`** — a tab strip over several tabs, one visible (`ActiveTab`). Asks
  **`IPaneHost`** (implemented by `ShellViewModel`) for anything involving its neighbours, so layout
  mutation lives in exactly one place.
- **`ShellViewModel`** — everything shared: the folder tree, bookmarks, browse settings, MFT status,
  the transfer/undo slot, the clipboard, and the layout. `ActiveTab => ActivePane.ActiveTab` is what
  the window chrome binds through; it raises `PropertyChanged` for it when either the pane or its
  tab changes.
- **`BertBrowser.Core.Layout.LayoutTree`** — the pane arrangement, pure and unit-tested
  (`LayoutTreeTests`). A node is a pane or a split (orientation + children + star weights).
  Its invariants are the point: splitting inside a parent of the same orientation appends a sibling
  rather than nesting, closing collapses a split left with one child and flattens a same-orientation
  nested one, no split ends a mutation with fewer than two children, and the last pane can't be
  closed. If you change it, mutate a rule and confirm a test goes red.

Views mirror that: `Views/DirectoryTabView` (address bar, search box, `ListView` + its `GridView`
and `ContextMenu`), `Views/FilePaneView` (tab strip + a `Grid` holding **every** tab's view, only
one `Visible`), and `Views/PaneLayoutHost` (nested `Grid`s + `GridSplitter`s, built in code because
`ColumnDefinitions` aren't bindable and the splitters aren't items of any collection).

Three things are easy to get wrong here:

- **A `GridView` and a `ContextMenu` are element instances and cannot be shared between
  `ListView`s** — they are declared inline in `DirectoryTabView.xaml`. Styles and `DataTemplate`s
  *can* be shared, which is why the thumbnail-view resources live in `Resources/Styles.xaml`.
- **A `TabControl` would be wrong.** It has one `ContentPresenter` and rebuilds the content on every
  switch; the file list's selection and scroll position live in the `ListView`, so switching tabs
  would lose both and re-realize the whole virtualized list.
- **Background work must not steal focus.** `FileListViewModel.Items` is replaced wholesale on every
  load and the view focuses the list on that change, so `DirectoryTabView.FocusFileList` is gated on
  `Tab.IsActive && IsKeyboardFocusWithin` — otherwise a directory finishing its load in one pane
  yanks the caret out of another pane's search box.

Anything that changes a directory on disk must **fan out**: `ShellViewModel.RefreshTabsShowingAsync`
reloads every tab showing one of the affected folders (matched via `PathKey`, never string
comparison), and `RefreshAllTabsAsync` covers a settings change. Transfers, undo, and paste all go
through it — a move from one open folder to another has to update both, not just the pane the drag
started in.

Keyboard: window `InputBindings` bind through `ActivePane`/`ActiveTab` (navigation, Ctrl+T/W/Tab,
Ctrl+1..9, Ctrl+Alt+arrow to split, Ctrl+Shift+W, F6 to cycle panes). Anything that could collide
with typing stays in a `PreviewKeyDown` with a focus guard: Ctrl+Z at the window (skipped when
`Keyboard.FocusedElement is TextBoxBase` — there is one search box and one path box *per tab* now),
and Ctrl+C/X/V and Alt+Enter in `DirectoryTabView` gated on its own list having focus. Plain Tab
stays WPF focus traversal. F2 (rename) is on the file list's own `KeyDown`, which is enough — it
never fires while a search or path box has the caret.

The file list has two modes: normal directory listing, or — while the search box has a query — a flattened result list (`FileListViewModel.IsFlattened`) that also shows each hit's folder relative to the search root.

Thumbnail (tile) view is the footer zoom slider: `ThumbnailScale` 0 keeps the details list, anything
above switches to tiles whose **width** is `ThumbnailSize`. The height is `ThumbnailTileHeight`, that
width through `AppSettings.TileAspectRatio` — so the slider means the same thing at every shape. The
ratio is a `BertBrowser.Core.Models.AspectRatio` (parsed, not a raw string, and covered by
`AspectRatioTests`) because it is a hand-editable line of settings.json: `Parse` **never throws**, and
anything unusable — including a default-constructed `0:0`, which a struct always permits — resolves to
4:3 rather than handing WPF a `NaN` height. The Settings picker lists `AspectRatio.Presets` plus the
current value if it isn't one of them, or saving would silently discard a ratio typed in by hand. It
is a global setting, so `ShellViewModel.RefreshTileAspect` fans it out to every open list after the
dialog commits; it only re-lays-out, so unlike the other browse settings it reloads nothing.

The file list is a multi-select (`Extended`) `ListView`. Ctrl/Shift/Ctrl+A come from WPF; `Views/MarqueeSelector` adds the rubber band — pressing on empty space and dragging sweeps a rectangle (drawn as an `Adorner`) that selects everything it touches, with edge auto-scroll. It only hit-tests *realized* containers, so it pins the drag origin to the row it landed on and recomputes the anchor from that row's live position, treating the anchor as off-screen once it virtualizes away. Selection-driven work (folder-tree reveal, status-bar summary) must therefore tolerate churn: the tree reveal is skipped while `MarqueeSelector.IsDragging`, and the summary is coalesced to one dispatcher pass. The tree is shared by every pane, so reveals are routed through `ShellViewModel.RequestTreeReveal`/`ActiveLocationChanged`, which drop anything not from the active tab and are debounced in the window — that single-writer rule is what stops open panes fighting over the tree's selection, expansion and scroll position. `PropertiesViewModel` takes a whole `IReadOnlyList<PropertiesTarget>` — with several targets it shows aggregates and three-state attribute checkboxes (indeterminate = the items disagree, and Apply leaves that bit alone).

Drag-and-drop is split three ways, and the split is load-bearing. `Views/DropPipeline` holds the
shared decide/highlight/execute logic; `Views/FileDragDropController` is attached **per tab** (drag
source + list drop target), which is what makes dragging between panes work — it resolves an
empty-space drop against its *own* tab's directory; and `Views/TreeDropTarget` is attached **once**
by the window, because the folder tree is shared and a per-pane controller hooking its `Drop` would
plan and carry out the same transfer once per open pane. Two further details matter: pressing an
already-selected row **defers** WPF's collapse-to-one-item selection to mouse-up, or a multi-item
drag would carry one item; and the plan computed while hovering is only advisory — the drop always
re-plans from scratch before writing.

**Dragging out to other applications** works, and the asymmetry is deliberate: the payload carries
CF_HDROP *as well as* the private `BertBrowser.FileItems` format, but `DropPipeline` **reads only
the private one**. So an in-app drop is decided by exactly the code and plan it always was, while
Explorer, editors, browsers and mail clients see an ordinary file drop. Accepting drops *from* other
applications is out of scope — UIPI blocks it because this app is elevated, and the workaround opens
a channel from lower-integrity processes.

The source sets `Preferred DropEffect` = `Copy`. That is documented as a clipboard-paste convention,
but Explorer honours it during a drag too — verified: a same-volume drag that would otherwise have
defaulted to Move came back as Copy — and copying is the right default for dragging a file into
another application. Shift still overrides it.

**Whether the originals are then ours to delete is the dangerous question**, and it is answered by
`Core/Services/Transfer/DragOutContract`, a pure function with a truth table in
`DragOutContractTests`. `DoDragDrop` returning `Move` does *not* mean "delete": Explorer's
same-volume move is an *optimized* move that relocates the files itself and reports
`PERFORMEDDROPEFFECT = None`. Only a non-optimized move puts the removal on the source. Both report
formats are frequently absent (Explorer left both unset in every drop this was verified against), so
the rule has a defined answer for "no report at all" — fall back to the returned effect — and never
deletes on anything except an explicit `Move`.

Two independent guards stop us acting on **our own** drops, either of which suffices:
`Views/DragSession` (a static claimed by `DropPipeline` the moment it recognises the private format)
and `DropPipeline` setting `e.Effects = None` after handling. Without them our own move reads as an
external one and we delete the items we just placed — the code previously never assigned `e.Effects`
at all, so `DoDragDrop` returned whatever WPF left there.

The removal itself goes through `ShellViewModel.RemoveDraggedOutSourcesAsync` → the **ordinary
reversible delete**. Nothing calls `File.Delete`. That means an external window's say-so cannot
reach past `DeletePlanner`'s refusals, sources that have already gone are dropped rather than
reported (that is what an optimized move looks like from here), and Ctrl+Z puts everything back.

This chain cannot be unit-tested past `DragOutContract`. It was verified with a scratch harness
built twice from one source — `asInvoker` and `requireAdministrator` — confirming a medium-integrity
Explorer really can call `GetData` back into a high-integrity process. Rebuild that harness rather
than trusting a fake, and note the two traps it hit: logging from inside the COM callbacks hangs the
app (they arrive on the UI thread inside `DoDragDrop`'s modal loop), and launching the `asInvoker`
build from an elevated shell silently gives it a high token, so there is no control unless it goes
out through `explorer.exe`.

The left sidebar has two sections: **Bookmarks** (top, sized to content) and **Drives & devices** (below, fills the rest). `FolderTreeViewModel.Roots` is `ObservableCollection<ISidebarNode>` mixing browsable `DirectoryNodeViewModel` drives (expandable tree) with `PortableDeviceNodeViewModel` leaves — MTP phones/cameras enumerated off-thread via `Interop.PortableDevices` (Shell.Application COM on an STA thread) that open in Explorer on double-click, since their contents aren't a filesystem path. Bookmarks persist in the `bookmark` table via `BookmarkRepository`/`IBookmarkService`; the file-list and tree context menus toggle them (`ShellViewModel.ToggleBookmarksAsync`), and `BookmarksViewModel` keeps an in-memory key set so the menu can label Bookmark/Remove without a DB hit.

Both sections honour "Show hidden items", and both filter rather than re-query: `BookmarksViewModel`
keeps every bookmark in `_all`, `DirectoryNodeViewModel` keeps every loaded child in `_allChildren`,
and the collection the view binds is the filtered projection — so `ShellViewModel`'s toggle re-filters
in memory and a subtree that was hidden comes back with its expansion state intact. The expander is
part of it: `IFileSystemService.ProbeSubdirectories` answers *any* and *any non-hidden* from one scan,
so a folder whose only subfolders are hidden doesn't offer an expander that opens onto nothing, and
the answer flips with the setting without touching disk again.

Each tree row shows its recursive size, small and dimmed, right of the name. **Nothing scans for it**:
a folder's number is a batched `DirSizeRepository.GetMany` over the siblings just populated, reading
the `dir_size_cache` rows the MFT pass already wrote for every directory on the volume, and a drive
root's is `TotalSize - TotalFreeSpace` (its own cache row would miss whatever sits outside the walked
tree). Unknown means blank, never zero — that is what a non-NTFS volume or a still-indexing one looks
like, which is why `ShellViewModel.OnMftIndexRefreshed` calls `FolderTreeViewModel.RefreshSizesAsync`
once for the shared tree: an already-expanded subtree fills in without being collapsed and reopened.
The size text is dimmed with `Opacity`, not a muted brush, so it stays legible on a selected row whose
foreground it inherits. Because that text is right-aligned, the tree can't let a scrollbar take
layout space: `SidebarTreeStyle` (in `Controls/Lists.xaml`) retemplates the `ScrollViewer` so the bar
**overlays** a gutter the rows always leave — the tree's right `Padding`, which must stay equal to the
`ScrollBar` style's 12px width. The pinned row carries the same 12px as a `Margin`, since it sits
outside the scroll area. That style is vertical-only on purpose; a tree that scrolls sideways would
clip with no bar to reach the rest.
