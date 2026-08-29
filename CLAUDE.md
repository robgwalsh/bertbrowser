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

# Drive the real window offscreen — see "Never launch the GUI" below.
tools/BertBrowser.Harness/bin/Debug/net10.0-windows/BertBrowser.Harness.exe --script tools/ui/smoke.bbs
```

`Directory.Build.props` sets `TreatWarningsAsErrors` and `Nullable` for all projects — any warning fails the build.

If a build fails with MSB3021/MSB3026 because a running BertBrowser instance locks `bin\Debug`, just kill it (`Get-Process BertBrowser | Stop-Process -Force`) and rebuild — no need to ask or build to a scratch directory.

## Never launch the GUI

`dotnet run --project src\BertBrowser.App` — and `BertBrowser.exe` — put a real window on the
user's screen, take keyboard focus, and compete with whatever they are doing. They multitask while
you work. **Do not run either, ever**, including "just to check something quickly". The only
exception is the user explicitly asking you to launch it.

To see the interface, use the harness (below), which hosts the same window where nobody can see it.

## Structure

- `src/BertBrowser.Core` — plain net10.0, no UI dependencies: SQLite persistence, path canonicalization, search/size services. This is the only project with tests; keep anything testable here rather than in the App.
- `src/BertBrowser.App` — WPF shell. MVVM via CommunityToolkit.Mvvm source generators (`[ObservableProperty]`, `[RelayCommand]` on `partial` classes), DI via `Microsoft.Extensions.DependencyInjection`. The composition root is `App.BuildServices()` in `App.xaml.cs` (`App.Services`): register new services/repositories there.
- `src/BertBrowser.Indexer` — the elevated index helper: a small `net10.0` console exe with a `requireAdministrator` manifest, the only component that asks for one. It hosts the existing `MftIndexService` and reports over a pipe; see "The elevated index helper" below.
- `tests/BertBrowser.Core.Tests` — xUnit; tests create real SQLite databases and directory trees under `%TEMP%`.
- `tools/BertBrowser.Harness` — the UI harness; `tools/ui/*.bbs` are its scripts.

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

`Core/Services/Transfer` is the only code that relocates user data — drag-and-drop, the harness's
`move`/`copy`, **and Ctrl+V**, which goes through `PlanDrop`/`ExecuteDropAsync` like a drop does.
There is exactly one implementation to audit, and the old parallel `FileTransferService` is gone
along with the paste path that had no progress, no undo and no conflict handling. It is split
deliberately:

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
- **`IFileCopier`** moves the bytes of one file, and is a seam for the reason `ITransferProbe` is:
  it is what lets `TransferExecutorTests` land a cancel *in the middle of a file* deterministically
  and in milliseconds instead of writing a multi-gigabyte fixture and racing it.

**Progress is byte-level, and a cancel takes effect inside a file.** Both come from one decision:
`FileSystemFileCopier` goes through `CopyFileExW`/`MoveFileWithProgressW` (`Core/Interop/CopyNative`)
rather than `File.Copy`/`File.Move`. The progress routine gives per-chunk byte counts, returning
`PROGRESS_CANCEL` stops a copy part-way, and staying on the OS call keeps everything `File.Copy`
already did — it *is* `CopyFile2` underneath — sparse files, SMB server-side copy, attribute and
timestamp semantics. A managed stream loop would have had to re-implement all of that, slower.
Three things around it are load-bearing:

- **The byte total is a lookup, not a walk.** `TransferEstimator` totals a plan from
  `dir_size_cache` through `ITransferSizeSource` (`App/Services/IndexedTransferSizeSource`, one
  batched `GetMany`), because sizing a 200,000-file tree before starting would cost more than the
  transfer. **A missing row is unknown, never zero** — the estimate comes back `Complete: false`,
  and the surfaces then show bytes and throughput against an *indeterminate* bar with no percentage
  and no time remaining. A determinate bar pinned at 0% reads as a stall rather than as an
  unmeasured volume. `TransferEstimator.MovesBytes` is the other half: **a same-volume move is a
  rename and costs nothing**, so it contributes no bytes and shows no bar — it asks
  `TransferExecutor.SameVolume`, the executor's own predicate, so the two cannot disagree.
- **`Run` coalesces.** `CopyFileEx` calls back thousands of times for one large file; item
  boundaries always report, in between at most one report per 100 ms — the guard `SearchService`
  puts on live results. It also counts bytes per file and *snaps* to the size already known at the
  end of one, which is what keeps the running total monotonic and exactly right at the finish.
- **A cancel leaves nothing half-written.** Between items, the rest are untouched; inside a file,
  the OS call removes the partial destination and keeps the source; inside a directory copy the
  partial tree goes through `DirectoryRemoval.RemoveTree` (a copy is defined as purely additive, so
  a cancelled one must add nothing); a cross-volume directory move takes the existing
  `TryDeletePartialCopy` path and **never deletes the source**. Whatever got across before the
  cancel stays across, `TransferOutcome.Cancelled` says it happened — without it a cancelled run is
  indistinguishable from an empty plan — and a cancelled move is still undoable. Staging moves and
  `Undo` run on `Run.Silent()`, uncancellable on purpose: a cancel landing half-way through putting
  a displaced entry back would strand it.

The surface is `TransferProgressViewModel`, shared by the status-bar strip in `MainWindow.xaml` and
the modeless `Views/TransferProgressWindow` behind its **Details…** link, so the two cannot drift.
It hangs off `ShellViewModel.TransferProgress`, **nullable and bound through `NullToCollapsed` the
way `IndexingStatus` is, deliberately not gated on `IsTransferring`** — that flag is what
`UiSession.Settle` waits on, so a posed transfer would hang the harness. Closing the detail window
does not cancel: unlike `DeleteDialog`, whose survey nothing depends on, the transfer outlives it.

Undo (Ctrl+Z, one level) reverses the last move and restores anything a Replace displaced.
`ShellViewModel.RetireUndoable` is what finally commits a replacement, so staged data outlives the
undo record by exactly one operation. Copy has no Replace and no undo: it is defined as purely
additive. The undo slot is **shared with rename** — one level, whichever happened last — so both
sides call `RetireUndoable` before claiming it, and `IsTransferring` gates both.

Tests are the point of this design — `TransferPlannerTests` (rules, fake filesystem),
`TransferExecutorTests` (real files, contents asserted; byte totals, mid-file and mid-tree cancels
through a `SteppedCopier`, plus a few against the real `FileSystemFileCopier`),
`TransferEstimatorTests` and `TransferRateTests` (pure, and the rate takes its timestamps as
arguments so it never sleeps), and `TransferRoundTripTests` (property tests: the multiset of file
contents under the root is invariant under any move, and undo restores the tree byte-for-byte —
these are what stand behind a rewritten copy path, since a copy that truncates fails them at once).
Two meta-tests assert those invariant checks can actually detect a lost or moved file. If you change
this code, mutate a rule and confirm a test goes red: make the estimator count a same-volume move,
or drop the partial-tree cleanup on a cancelled copy, and one goes red on its own.

`tools/ui/smoke.bbs` covers the surfaces with `progress-demo`, which **poses** them at fixed numbers
rather than running a transfer — one fast enough to be safe here is over before a capture could
catch it, and a slow one would put a different throughput figure and time remaining into every
picture. `progress-demo unsized` poses the degraded shape; `dialog transfer` photographs the detail
window, and `themes.bbs` does it in both palettes.

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

### Creating

`Core/Services/NewItem` is split the same way again, and is the smallest of the three because
there is only ever **one** item: every entry point names one thing, so `NewItemPlan` carries one
`RejectedNewItem?` rather than a list, and there is no batch for one item's failure to cost.

**Creating is additive, exactly as copying is, so it has no undo and does not touch the
three-way slot.** `CreateNewItemAsync` never calls `RetireUndoable`, which means a rename or a
delete before it stays undoable — the item it made is empty, and Delete removes it reversibly.
`NewItemOutcome` deliberately has no `CanUndo` member at all; the absence is the design.

- **`NewItemPattern`** is the naming rule, pure and shared. Its character and reserved-name checks
  are **`RenamePattern.Validate`'s** — a name Windows refuses refuses it whether the file arrives by
  rename or by creation, and `NewItemPlannerTests` asserts the refusal comes back in *that*
  function's own words, so re-implementing it goes red. It cleans trailing dots and spaces before
  validating, since Windows drops them silently and "Reports. " is a perfectly good request for
  "Reports". Its one addition is measuring the typed stem and the type's extension **together**
  against the 255 limit, because the box only holds half of what lands on disk.
- **`NewItemPlanner`** decides, touching nothing, through **`INewItemProbe`**, and exists for the
  reason the rename planner does: the dialog asks on every keystroke, so the rule it previews is the
  rule the create obeys. Four refusals, each something the dialog can say while the name is still
  editable — `ParentMissing`, `InvalidName`, `NameTaken`, `TemplateMissing`. **`ProtectedLocations`
  is deliberately not consulted**: it guards a few folders against being *deleted*, exact-match only,
  and creating inside them is the ordinary thing this app is for — a new file in the profile root is
  not a mistake, and `C:\Windows` is refused by its ACL now the app is `asInvoker`.
- **`NewItemExecutor`** writes, re-applying those rules against live disk first. **The trap it is
  shaped around is that `Directory.CreateDirectory` succeeds silently on a folder that is already
  there** — a create that merely reported success would hand the user somebody else's folder — so
  existence is checked immediately before the call rather than trusted from a plan built while the
  dialog sat open. Files need no such care and do not get it: `FileMode.CreateNew` and
  `File.Copy(overwrite: false)` both throw on a taken path, which closes the window a
  check-then-create leaves open. A copied template has Hidden/ReadOnly cleared, because the shipped
  ones under `%APPDATA%\Microsoft\Windows\Templates` are commonly both and the file the user asked
  for should be neither.

**The `"name (2)"` rule is now one function**, `Core/Paths/UniquePath`, with three callers — the
transfer executor's staging, the delete executor's, and the dialog's suggested name. Its
`isDirectory` is a **parameter, not something probed from the path**: the two executors are making
room for an item already at that path, but the dialog is placing something new, and probing would
tell it about whatever is *in the way* instead — a folder named `notes.txt` blocking a new file
`notes.txt` would give `notes.txt (2)` rather than `notes (2).txt`. `UniquePathTests` covers exactly
that.

**The file types are the app's own list, not Windows'.** `AppSettings.NewFileTypes` is nullable on
purpose, the same distinction `ThemeId` draws: null means never configured and ships
`NewFileTemplate.Defaults()`, `[]` means the user emptied it deliberately. `ResolvedNewFileTypes` is
the one place that resolves it, so the menu, the settings page and the harness cannot disagree.
"New ▸ Folder" is not in the list and is never configurable.

**Windows' `ShellNew` registry is read, never written.** Adding a per-user entry needs no elevation
but *removing* a machine-wide one does, which would mean registry-write verbs on
`BertBrowser.Indexer` and undo the point of the four-verb elevated surface — and it would change
Explorer's own New menu machine-wide. So `Interop/ShellNewRegistry` (App) opens every key
`writable: false` and emits raw values, and **`ShellNewImport` (Core) does all the deciding**, which
is what lets `ShellNewImportTests` cover it in a project that cannot open a registry key. Two rules
there are load-bearing: **`Command` entries are dropped**, because they name a program to run (it is
how Shortcut and Briefcase work) and honouring one would put a registry-supplied command line
through `ProcessLauncher`; and a label that is an unresolved `@dll,-id` resource string falls back to
the bare extension rather than reaching the menu as its own raw text. The three ShellNew value kinds
collapse to **two** on the way in — `Data`'s bytes are written out once, at import, into
`AppPaths.TemplatesDir` — so every persisted template is either "empty" or "a file on disk" and the
executor has one branch instead of three.

`Views/NewItemDialog` is a near-copy of `RenameDialog` and deliberately so, down to the
`internal static Create` the harness needs. It re-plans on every keystroke, and again in `Ok_Click`
because the disk may have changed while it sat open. `Views/NewItemMenu` builds the file-type
entries the way `CustomCommandMenu` does — `Tag`-based removal, `SetResourceReference` so runtime
items follow a theme change, `"_"` doubled — while "Folder" and "Empty file…" stay in XAML either
side of the anchor.

Two placement details. **New targets the folder being shown, never the selection**, which is why it
sits at the top of the file list's menu — and why an empty-space right-click, which opens that same
menu, gets it without needing its own affordance. And it is **disabled while the list is flattened**:
a search result is not a folder, and creating into the search root would make something that may not
match the query and so would not appear, which reads as a failure. Ctrl+Shift+N lives in
`FileList_KeyDown` beside F2 and Delete, with its own modifier guard, because it acts on the focused
pane's directory.

`ShellViewModel.CreateNewItemAsync` sets **`PendingSelection` itself**, before awaiting the refresh,
rather than leaving the view to select afterwards — so the tree's New lands in whichever pane is
showing that folder, and the harness's `newfolder` exercises the same selection path the menu does,
which is what makes `assert-selected 1` in a `.bbs` a real assertion. `RefreshAfterCreateAsync`
rebuilds the tree **only for a folder**: the tree shows nothing else, and a rebuild costs containers,
which is most of what the folder-tree rules are about.

Tests: `NewItemPatternTests`/`NewItemPlannerTests` (rules, fake probe at the bottom of the file),
`NewItemExecutorTests` (real files, contents asserted, with a meta-test that the "exactly these
names" check can fail), `ShellNewImportTests` (the mapping), `UniquePathTests`. `tools/ui/newitem.bbs`
covers the wiring — including a rename, then a create, then `assert-can-undo`, which is what proves
the create left the undo slot alone. Mutate a rule and confirm a test goes red: drop the executor's
existence check and `AFolderThatAppearedSinceThePlan_FailsRatherThanBeingAdopted` goes red on its own.

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
  the check standing between a mangled outcome and erasing a tree it was never given.
- **Nothing here erases a tree with `Directory.Delete(recursive: true)`; it all goes through
  `Core/Services/DirectoryRemoval`.** That call cannot be used on anything the user owns: given a
  junction anywhere in the tree it erases everything *else* and then throws
  `ERROR_INVALID_PARAMETER` naming the link. On a permanent delete that meant the contents were gone,
  unrecoverably, and the user was told the delete had failed; on a staging commit the throw is
  swallowed as harmless cleanup and half a folder stays for good. `RemoveTree` walks with an explicit
  stack, collects directories in pre-order and removes them in reverse, and takes a junction as the
  one entry deleting it removes. `TransferExecutor.CommitStaging` uses it for the same reason — what
  a Replace displaced is the user's own folder. `DirectoryRemovalTests` covers it; make the walk
  follow a link and it goes red.
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

### The elevated index helper

**The app is `asInvoker`.** Exactly one thing needs an administrator token — `MftVolumeIndexer.Open()`
calling `CreateFileW(@"\\.\C:")` — and that one thing lives in its own process,
`src/BertBrowser.Indexer` (a `net10.0` console exe with a `requireAdministrator` manifest). The App
registers `MftIndexClient` for `IMftIndexService` instead of `MftIndexService`, so every consumer —
`ShellViewModel`, `DirectoryTabViewModel`, `SearchService` — is unchanged by the split. That
six-member interface is the whole seam.

Do **not** put `requireAdministrator` back on the app to fix an access-denied error. A folder this
app cannot read is a folder Explorer cannot read either, and that is now the intended behaviour.

Four decisions here are load-bearing, and three of them were only found by driving the real thing:

- **The app is the pipe server and the helper is the client**, which is the reverse of the obvious
  arrangement. A pipe created by a high-integrity process carries a High mandatory label, and
  mandatory policy forbids writing *up*, so a medium-integrity app could not write to a pipe its own
  helper had created. Creating it app-side makes the helper's connection a write-*down*, always
  permitted, and no labelling code is needed. Verified: a Medium (`S-1-16-8192`) process created the
  pipe, a High (`S-1-16-12288`) helper connected, and messages crossed both ways.
- **`PipeOptions.Asynchronous` on both ends, and real buffer sizes on the server.** Two separate
  deadlocks live here. A pipe created with zero-size buffers holds nothing, so every write blocks
  until the peer reads — and with both ends greeting each other, neither ever does
  (`SingleInstance` gets away with zero only because its pipe is one-directional and never written
  to). And Windows serializes I/O on a *non-overlapped* handle, so the helper's main thread parked in
  a blocking read blocks its own volume threads' writes on the same handle: not one progress message
  could leave while the app sat waiting for exactly those messages. Both look like "the helper
  hangs".
- **`CanElevate` must not use `IsInRole(Administrator)`.** An administrator running normally holds a
  *filtered* token in which the Administrators group is deny-only, so `IsInRole` answers **false**
  for precisely the people who can elevate — the app would tell every one of them their account is
  not an administrator and never offer the prompt. `ElevatedIndexHostLauncher` reads the token's
  elevation type instead (`Limited` or `Full` ⇒ can elevate). Measured on a real medium-integrity
  process: `CanElevate: True`, `IsInRole(Admin): False`.
- **The elevated surface is four verbs and never a path** — `Hello`, `Start`, `Shutdown`, `Ping`.
  `IndexProtocol.IsAcceptableArgument` refuses an argument on any of them. Adding a "re-index this
  folder" verb would put an attacker-chosen path on the elevated surface and undo the point of the
  split.

Lifetime: **losing the pipe is what kills the helper**, and the kernel breaks it however the app
ends, crash included — measured at 4 ms after a `taskkill /f` mid-index. A watchdog on the parent's
process handle is the backstop, and `Shutdown` is only the tidy path. The app **cannot** terminate
the helper (a medium process may not open a high one for `PROCESS_TERMINATE`), which is why the
first two are the guarantee.

Both processes share the SQLite database, and that is safe for a reason worth knowing: the profile
directory carries inheritable Full Control for the interactive user, so a file the elevated helper
creates is *owned* by Administrators but still grants the user full access. Verified with `icacls`
after a real 295,808-row index pass. The app creates and migrates the database first and the helper
opens it with `create: false` and never calls `Migrate()` — schema ownership stays with the app.

Degraded mode is the normal case, not an error: a declined prompt, a standard-user account, or a
dead helper each leave `IsIndexed` false, `SearchService` falls back to its crawl exactly as it does
on a non-NTFS volume, and the status bar carries the reason plus a retry. **Nothing retries on a
timer** — every retry is a UAC prompt.

Packaging costs nothing: CI publishes the helper into the same `publish` folder the app goes to, and
`vpk pack --packDir publish` picks it up. Measured at ~218 KB on top of a 150 MB publish, because the
helper's `net10.0` runtime set is a strict subset of the app's.

**A local build needs the same thing, and the way it fails is misleading.** `ElevatedIndexHostLauncher`
resolves the helper beside the executable and nowhere else, so a `bin` without it fails `File.Exists`
*before* `ShellExecuteEx` — the status bar reports the index unavailable having never raised a UAC
prompt, which reads as an elevation problem and is not one. `BertBrowser.App.csproj`'s `CopyIndexHelper`
target builds the helper and copies it across, and two things about it are deliberate: it invokes the
build directly rather than through a `ProjectReference`, because even `ReferenceOutputAssembly="false"`
pulls the helper into the app's *publish* graph where it lands incoherent — apphost and `.json` files,
no managed `.dll` — and is then hidden by the real self-contained publish landing on top of it; and it
passes `RemoveProperties`, because a nested build inherits the outer one's global properties and
`dotnet publish -r win-x64` would otherwise demand a RID flavour of the helper that nothing restored.
Since the copy is not a reference, it does not propagate to projects referencing the App — a scratch
driver that wants the real chain must copy the helper itself.

### Launching other programs

**There is exactly one `Process.Start` in the app, inside `ProcessLauncher`**, and a `git grep`
finding a second is still a bug — but the reason is now ordinary hygiene rather than danger. This
process no longer holds an administrator token, so a child inherits an ordinary one; the chokepoint
survives because one place that starts programs is one place to audit, to fake in the harness, and
to change.

`ProcessLauncher` is `Process.Start` with `UseShellExecute` and, when the user asked for it, the
`runas` verb — which from a medium-integrity process raises a real UAC prompt, so "Run as
administrator" finally means what it says. `ERROR_CANCELLED` (1223) is reported as the user's choice
rather than a failure.

**`Interop/ShellLauncher.cs` is gone**, along with its hand-declared COM vtables and the
`AllowSetForegroundWindow` dance. It existed only to reach `explorer.exe` and borrow a lesser token
back, and both of its reasons vanished with the manifest change. Do not reintroduce it.

**`Core/Services/ExecutablePath` stays.** Its first justification is dead (`Process.Start` throws
when a program is missing, where `ShellExecute` returned `void`), but its second is not: it decides
*what* runs against `PATH` rather than leaving a bare name to resolve against the folder being
browsed. `ExecutablePathTests` covers it, including that a path which isn't fully qualified is never
probed.

Elevation is offered three ways: the file list's **Run as administrator** item (one file only),
Ctrl+Shift+double-click / Ctrl+Shift+Enter, and a per-command checkbox on custom commands. The
plain-Enter arm of `FileList_KeyDown` needs its modifier guard or it swallows Ctrl+Shift+Enter
first, and the double-click handler resolves the row **under the cursor** rather than
`SelectedItem`, because Ctrl+Shift has already told an `Extended` `ListView` to range-extend. The
folder tree deliberately has no elevated entry — a drive root is one careless click away there.

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
  check. Both peers are ordinary medium-integrity processes of the same user, so the pipe needs no
  mandatory-label work — a DACL admitting only the current user's SID is right, and the client's
  identity is re-checked after connect anyway. **A failed hand-off falls through and starts
  normally**: the first copy may be mid-shutdown, and starting is better than exiting having done
  nothing. Its framing and identity check now come from `Core/Ipc` (`LineChannel`/`LineReader`,
  `PipeIdentity`), shared with the index-helper pipe — but note its pipe is one-directional and the
  server never writes, which is why zero-size buffers are safe there and are not for a duplex pipe.
- **The endpoint name is random, and that is not decoration** (`Core/Ipc/InstanceEndpoint`). Pipe
  names are one machine-wide, first-come namespace with no per-user partitioning, so the old
  predictable `BertBrowser.<SID>` was a name *another signed-in account* could take first — after
  which the real first copy could never create its listener, and every launch wrote the folder path
  it was asked to open to the squatter and exited having opened nothing. A DACL cannot answer that:
  it governs who may open an endpoint, not who may claim the name. So the name carries a 128-bit
  nonce, and the copy that owns it publishes the name to `~/.bertbrowser/instance.pipe` — protected
  by the profile's own permissions — for the next launch to read, withdrawing it on dispose. The
  **mutex** stays the "am I first" gate, because `Local\` *is* per-session and cannot be squatted.
  Two things to keep: `Publish` must not `CreateDirectory` (`AppPaths.MigrateLegacyData` only runs
  when the data directory does not yet exist, so creating it here would silently retire that
  migration), and a missing or stale file must fail the hand-off rather than throw — the caller then
  starts normally, which is the existing fallback. `InstanceEndpointTests` covers the name rule.

The protocol is deliberately one verb — *navigate to this path*. Nothing on the wire can become a
launch, a file that gets written, or anything but a directory listing.

**Two traps here fail silently, and both were caught only by driving the real app:**

- **`GetImpersonationUserName()` returns the bare account name** ("Rob"), while
  `WindowsIdentity.GetCurrent().Name` is qualified ("MACHINE\Rob"). Comparing them whole never
  matches, so the server accepted every connection and immediately dropped it — the hand-off looked
  like it worked, and a second full copy started anyway. `SingleInstance` compares the account
  portion. The DACL is the real gate; this check is defence in depth and must not be allowed to fail
  closed by accident.
- **A pipe that connects and then breaks means the server dropped you**, not that the pipe is
  missing. `Connect()` succeeding followed by "Pipe is broken" on the first write is the signature of
  a server-side rejection, and is worth checking before assuming a naming or ACL problem.

`ShellViewModel.OpenRequestAsync` carries a request out, resolving each target against disk (a
target naming a file opens its folder and highlights it, which is the only useful reading of "open
this file" for a file browser). Highlighting goes through `DirectoryTabViewModel.PendingSelection`,
a one-shot the **view** consumes — the selection lives in the `ListView`, so nothing else can apply
it. Three things about it are load-bearing, and getting any of them wrong means `/select` quietly
selects nothing:

- **It is set *before* the navigation is awaited**, not after. The listing that arrives is the one
  the selection belongs to, and by the time an `await` returns the view has already been told.
- **It is observable, and applied on either signal** — the property changing *or* the listing being
  replaced. Selecting something in the folder already open is the common case, and there no reload
  is coming to trigger it later.
- **The apply is deferred to `DispatcherPriority.Background`.** Both signals arrive before the
  `ListView`'s `ItemsSource` binding has caught up with the view model's new collection, so
  selecting immediately searches the folder that was showing a moment ago.

It clears on success; a failure only clears when the listing it was waiting for has actually
arrived. Applied *before* `FocusFileList`, since selecting scrolls.

### The preview pane

Docked right of the file list **inside `DirectoryTabView`**, per tab, behind a `GridSplitter`.
Per tab because the address bar, search box, progress bar, error banner and — decisively —
`SelectedItems` are all already per tab. Ctrl+P toggles it; Alt+P does too, for Explorer's muscle
memory, and has to live in `MainWindow`'s `PreviewKeyDown` because an Alt chord arrives as
`Key.System` with the real key in `SystemKey`.

The split follows the house shape: the deciding and the parsing are pure and in
`Core/Services/Preview` where xUnit can reach them; the App does the I/O and the pixels. **Nothing
is registered in `App.BuildServices()`** — `PreviewPaneViewModel` is constructed by
`DirectoryTabViewModel` exactly as `FileListViewModel` is, from dependencies that ctor already has,
and the Core services are static functions.

Five rules are the design, and each is a thing Explorer's pane gets wrong:

- **No file is ever held open.** Every read opens with `FileShare.ReadWrite | Delete`, copies into
  memory and closes *before* anything is decoded, so previewing never blocks renaming, moving or
  deleting — which in this app would mean blocking its own executors. The single exception is
  deliberate and visible: pressing play hands the path to a `MediaElement`, which owns it until the
  selection moves on (`StartOrStopMedia` calls `Close()`). `tools/ui/preview.bbs` proves the rule by
  previewing a file and then renaming, deleting and undoing it; break the sharing flags and it goes
  red.
- **Nothing blocks the UI thread**, including `File.GetAttributes` — on a dead network share that is
  the call that hangs. One CTS per request, cancel-previous, `OperationCanceledException` swallowed,
  cancelled in `Dispose`; its *own* CTS, so a listing refresh cannot cancel a preview or the
  reverse. The refusals that need no disk (nothing selected, several selected, a folder) are
  answered on the UI thread instead, so those never flash a spinner on the way to a one-line
  message.
- **Selection churn is free.** 150 ms debounce, and skipped outright while `MarqueeSelector.IsDragging`
  — a rubber band adds and removes items one at a time and would otherwise start a file read per
  row. The push rides `QueueSelectionSummary`'s existing coalescing. `MarqueeSelector.DragEnded`
  exists for this: the *last* selection change of a sweep happens while the band is still down, so
  without it the pane would keep showing whatever was selected before the drag.
- **Every read is bounded** — text at `AppSettings.PreviewTextMaxBytes` and 5,000 lines, archives at
  1,000 entries, images decoded no wider than 2048 (and never scaled *up*, which is why the header
  is probed before the decode). An oversized image is downgraded to the shell rather than refused:
  the shell can thumbnail a 500 MB TIFF without us reading a byte.
- **A cloud placeholder is never hydrated.** `Offline`, `RecallOnOpen` or `RecallOnDataAccess` is
  `NotDownloaded` and a message, not a silent multi-gigabyte fetch. The two recall bits are absent
  from .NET's `FileAttributes` and are named on `PreviewClassifier`.

- **`PreviewClassifier`** decides, touching nothing: kind, byte budget, or a refusal. An
  unrecognised extension becomes `Document` — an honest attempt through the shell — rather than an
  immediate refusal, because the classifier cannot know which handlers this machine has. Office and
  OpenDocument extensions are deliberately **not** archives even though they are zips: the shell
  makes a real page-one thumbnail of them, which beats a listing of their guts.

  **The extension table is not the answer on its own, and must not be treated as one.** Its tail is
  endless, and every name missing from it was a file the pane refused for no reason the user could
  see — `choco.exe.manifest` is plainly XML, and the `.ignore` beside it is plainly a list. So a
  `Document` carries a **text budget**, and `PreviewPaneViewModel.BuildDocument` asks the shell
  first and then *reads the bytes* if the shell declines. The shell goes first because where it has
  a handler its answer is better (a .docx read as text is gibberish). Adding an extension to the
  table now only buys colouring and one saved round-trip — it is no longer what decides whether a
  file can be previewed at all. The one case that deliberately gets a **zero** budget is an image
  too large to decode: it is still an image, and reading a gigantic TIFF as text would be nonsense.
- **`TextPreviewReader`** is pure over a `Stream`: BOM, then strict UTF-8 validation, then a
  UTF-16-without-a-BOM heuristic (the one case where NUL bytes mean text), then **Latin-1** — chosen
  over the machine's ANSI codepage because it maps every byte, never throws, and gives the same
  answer on every machine, which is what makes the tests mean anything. Whether the read was
  truncated is passed *into* the UTF-8 validation: a sequence running off the end is evidence of the
  cut when there was one and evidence against UTF-8 when there wasn't.

  It answers **two** questions, and the pair is load-bearing. `TextPreview.LooksBinary` is "did this
  file with a text extension turn out to be binary?" — a NUL in the first 8 KB, which only has to
  catch the obvious case. `IsConvincingText` is the stricter one the document fallback uses, where
  we are *guessing*: it adds a control-character ratio, because a binary with no NUL in its first
  8 KB passes the loose check and then decodes as a wall of mojibake. Tab, newline and carriage
  return are not counted, so an indented file is not mistaken for rubbish. The messages differ for
  the same reason — "binary file" is a fact when a `.txt` isn't text, and a guess dressed as one
  when we opened the file on spec, where the honest answer is "no preview available".
- **`SyntaxTokenizer`** is hand-rolled and dependency-free, spans rather than a tree. The property
  that matters is not colour but the **cover**: ordered, gap-free, non-overlapping, never past the
  end — the view builds runs from it and would throw otherwise. `Merge` degrades to one plain span
  rather than return a cover that does not hold, and the tests assert the property over every
  language on deliberately malformed input.
- **`PreviewMetadata`** selects by **canonical** name (`System.Image.Dimensions`), never the
  localised label, or the strip silently empties on a non-English Windows. `ShellProperties` now
  carries the canonical name, from `IPropertyDescription.GetCanonicalName` — which was already
  declared in that vtable and unused.

`ShellThumbnails.GetPreview` passes **`SIIGBF_THUMBNAILONLY`**, unlike `GetThumbnail`: the shell
then declines instead of substituting the file-type icon, which is what lets the pane say "no
preview available" rather than blow a 32 px icon up to fill the panel. `PreviewImageCache` is a
bounded LRU keyed by path, size **and modified time** — a preview that outlived an edit would be a
lie.

In the view: a read-only `RichTextBox`, so the text can be **selected and copied**, which Explorer's
pane cannot do. The cost is one inline per coloured span, so colouring stops past 1,500 lines and
the footer says so — the text is still shown. The gutter is a single `TextBlock` translated by the
editor's scroll offset, and is hidden when wrapping is on, because a wrapped line is not one row.
**The chequerboard is built in code-behind and rebuilt on the token brushes' `Changed` event**: a
tiling brush caches its realisation and a `SolidColorBrush` inside one changing colour does not
invalidate it — the same trap documented for `VisualBrush` in the harness.

`Theme.Preview.Checker*` and `Theme.Syntax.*` are the only new tokens; everything else reuses
`Theme.Surface.*`/`Theme.Border.*`/`Theme.Text.*` rather than minting a parallel family.
`ThemeCatalogTests.Code_stays_readable_in_the_preview_pane` contrast-checks them against every
built-in — only the two roots define them, so a palette that reads on Dark+ and vanishes two shades
lighter is caught there (it caught Nord, Cobalt2 and Everforest on the first attempt).

Visibility is **per tab**; `AppSettings.ShowPreviewPane` is what a new tab starts from and toggling
writes it back. Width is **global** — panes differ in width, so a per-tab width reads as the
splitter moving on its own. `ColumnDefinition.Width` is not bindable, so `UpdatePreviewPane` assigns
it, the way `UpdateRelPathColumn` does.

Tests: `PreviewClassifierTests`, `TextPreviewReaderTests`, `SyntaxTokenizerTests`,
`ArchiveListingTests`, `PreviewMetadataTests`. `tools/ui/preview.bbs` covers the wiring, with
`preview-fixture` laying down files that are really what their extension says — the ordinary `tree`
fixture's `photo.jpg` is text, which is fine for a listing and useless for a preview. Mutate a rule
and confirm a test goes red: drop the classifier's placeholder check and the `NotDownloaded`
theories go, drop the tokenizer's string handling and the cover assertion does.

**Rendered Markdown and real PDF paging are the two things that want a package** (Markdig, PdfPig)
and are deliberately absent: Markdown previews as its coloured source, PDF as page one from the
shell.

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

Five things here are load-bearing and easy to undo by accident:

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
- **Selected text in a `TextBox` needs an MSBuild switch, not just brushes.** WPF has two ways of
  painting a selection and only one honours `SelectionTextBrush`: the default draws the selection as
  an adorner *over* the run, so at `SelectionOpacity="1"` it is an opaque rectangle covering the
  glyphs and `Theme.Input.SelectionForeground` is ignored outright — which reads as "the text goes
  the same colour as the highlight" and looks like a palette bug rather than a rendering one.
  `Directory.Build.props` sets
  `Switch.System.Windows.Controls.Text.UseAdornerForTextboxSelectionRendering=false`, which moves
  painting into `TextBoxView` — highlight behind the run, glyphs on top in `SelectionTextBrush`. It
  is in the shared props rather than one csproj so the app and the harness that photographs it can
  never disagree about how text renders. `ThemeCatalogTests` already contrast-checks that token pair,
  so every built-in was correct the whole time the switch was missing. **Note the harness cannot see
  this**: its window is never activated, and WPF paints no selection in an inactive one — it was
  measured with a scratch WPF app rendering an offscreen `TextBox` both ways.

Things that look done but aren't unless you check: `GridViewColumnHeader` needs `PART_HeaderGripper`
(or column resize breaks silently) and a blank template for `Role=Padding` (or a classic strip shows
after the last column); menu separators resolve through `MenuItem.SeparatorStyleKey`, not an
implicit `Separator` style; `TextBox` needs explicit `CaretBrush`/`SelectionBrush` (and the
selection-rendering switch above).
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

### The app icon

`src/BertBrowser.App/Assets/app.ico` is a build output, and `tools/icon/build-app-icon.ps1` is its
source — there is no `.svg` or `.psd` behind it. Change the drawing there and commit the regenerated
file (`powershell -NoProfile -ExecutionPolicy Bypass -File tools/icon/build-app-icon.ps1`; Windows
PowerShell, not `pwsh`, since the drawing is GDI+). `-PreviewDir` drops the frames out as PNGs,
which is the only way to judge them — the small sizes have to be looked at magnified, on a light
ground and a dark one. The mark is a folder whose face is the app's own layout: sidebar tree, a
splitter, two panes of name/size rows, one row selected in the accent colour.

**It is three drawings, not one scaled**, and that is the whole design. At 16px one device pixel is
16 units of the 256-unit design space, so every row, divider and rim narrower than that dissolves
into grey — a 256px drawing shrunk to 16 is a brown smudge. So there are three tiers (S 16/20/24,
M 32/40/48, L 64/96/128/256), each laid out on a grid that lands on whole pixels at its own base
size, each carrying only the detail its smallest member can hold: tier S has no sidebar and a much
larger amber margin, because at that size the *folder* has to be recognisable before anything in it
is. The file also carries 20, 24, 40 and 48 so that 125%, 150%, 200% and 300% scaling each land on a
frame drawn for that size rather than on a resample. Frames ≤48 are BMP and ≥64 are PNG — the
conventional layout, and the one `vpk --icon` reads.

**`Window.Icon` is deliberately never set, on any window.** A null `Icon` is not a missing icon: WPF
then lets Windows use the executable's own icon resource for the taskbar and Alt+Tab, and the shell
picks per size out of all ten frames. Setting it would replace that with one frame scaled to
everything — and WPF picks that frame itself, badly: `Icon="/Assets/app.ico"` in XAML resolves
through `BitmapFrame.Create`, which hands back the 64x64 frame whatever the surface needs.

So the title-bar icon goes through `Views/AppIcon` instead, which opens the `.ico` and chooses the
smallest frame at least as large as the slot — downscaled or exact, never blown up.
`ThemedWindow.ShowsAppIcon` is the opt-in (**`MainWindow` only**; the dialogs carry a title that says
what they are, as Explorer's do) and `TitleBarIcon` is the resolved frame the template binds. It is
re-picked on `WM_DPICHANGED`, because the frame that was exact on one monitor is not on the next.
Two things bite: the pack URI must be **assembly-qualified**
(`pack://application:,,,/BertBrowser;component/...`), since a relative one resolves against the
*entry* assembly and that is `BertBrowser.Harness` when the harness hosts these windows; and the
decode is wrapped, because it runs from a dependency-property setter during XAML parse, where an
exception surfaces as the window failing to open rather than as an icon going missing.

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
Explorer, editors, browsers and mail clients see an ordinary file drop.

Accepting drops *from* other applications is **now possible but not implemented**, and that changed
without anyone touching this code: UIPI used to block it because the app was elevated, and the app
is `asInvoker` now. WPF will therefore accept such a drop and `DropPipeline`, which reads only the
private format, will **silently ignore it** — a worse experience than the OS refusing outright.
Handling external `CF_HDROP` is a separate piece of work; until then this is a known gap rather than
a mystery.

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

**A tree row that loses its `TreeViewItem` takes the selection with it, and this tree reports a
selection as a navigation.** WPF answers the removal of the selected container by selecting its
parent, which cascades to the drive root — so toggling "Show hidden items" walked the active tab to
`C:\`, and a refresh after a move or a delete walked it up a level. There are two halves, because
there are two kinds of rebuild:

- **`RebuildChildren` never calls `Clear()`** — it diffs, removing and inserting only what actually
  changed. A folder that is merely being filtered in or out of view keeps its container, so the
  hidden-items toggle disturbs nothing.
- **`RefreshDirectoriesAsync` goes through `RebuildingAsync`**, which suppresses `DirectorySelected`
  — a repopulate genuinely builds new child objects, so there is no container to keep. The
  suppression **must outlive the call**: the teardown and selection fix-up happen on the *next
  layout pass*, so a guard that ends when the method returns catches nothing at all, and looks like
  it works because an assertion made straight afterwards runs before the stray selection has
  happened. Hence the release at `DispatcherPriority.Loaded`.

- **`NoteSelected` swallows the echo of a selection this class made itself.** Keeping the containers
  alive (the point of the diff) means an assignment to `IsSelected` is echoed back through the
  container a layout pass later — after the suppression has been released, and indistinguishable
  from a click. So the node is remembered and its next announcement ignored, once. The cost is at
  most one ignored click on the row the tree had just selected by itself.

There is deliberately **no attempt to put the selection back** after a refresh replaced the selected
row. Assigning `IsSelected` to restore it runs straight into the echo above, and re-announces
whatever the tree had settled on — with the tab in a folder the tree cannot reveal (anything under
`AppData`, which is hidden), that was the deepest reachable ancestor, and the tab jumped there on
startup. Leaving the tree unhighlighted until the next reveal is the cheaper half of the trade.

`tools/ui/tree.bbs` covers this — its `settle` calls are load-bearing, since an assertion made
immediately after the command passes either way, and its `tree-click` steps are what prove the
guards have not also swallowed a *real* selection. Put the `Clear()` back and it goes red at the
hidden toggle; drop the suppression and it goes red at the move.

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

### Verifying the interface (the harness)

`tools/BertBrowser.Harness` is how the UI gets looked at, and it exists because of one constraint:
**the user is at the machine while you work.** A window that appears over what they are doing and
takes the keyboard puts their keystrokes and the test's into the same queue. So the app is never
launched; the harness hosts the same `MainWindow`, from the same `App.BuildServices()` graph, on its
own STA thread — parked at -32000,-32000, `ShowActivated=false`, `WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE`,
`EnableWindow(false)` — and captures it with `RenderTargetBitmap`, a **software re-render of the
visual tree rather than a screen grab**, so where the window sits and what covers it are irrelevant
to the picture. `.claude/skills/verify` is the command reference; `tools/ui/*.bbs` are the scripts.

Five things here are load-bearing:

- **The composition root had to be split from the launch.** `App.BuildServices()` builds the graph
  and does nothing else; `OnStartup` keeps the side effects that belong to a real launch — the MFT
  indexer, the update check, `PurgeAbandonedStaging`, the single-instance listener, `window.Show()`.
  The harness wants the first and none of the second. `BuildServices` takes a `customize` callback
  so the harness can replace `IProcessLauncher`, and `App.UseServices` sets the static the
  code-behind reaches through.
- **The window is given a launcher that refuses**, because that is the one thing offscreen cannot
  fix: opening a file starts another program, and *its* window belongs to the desktop. For the same
  reason the harness never touches the clipboard (there is one, and the user is using it) — `move`
  and `copy` go through `TransferPlanner`/`TransferExecutor`, which is what paste and drag-and-drop
  go through anyway.
- **And it is given an index service that cannot prompt.** The app's own registration starts
  `BertBrowser.Indexer.exe` elevated, which puts a UAC dialog on the desktop — the same problem as a
  launched program, and worse, since it takes the secure desktop. `UiSession` injects
  `NullMftIndexService`, and asserts it did not resolve an `MftIndexClient` rather than trusting the
  registration to stay put. `--index` now means the **in-process** `MftIndexService`: identical
  behaviour to before, and on an unelevated run `MftVolumeIndexer.Open()` fails soft on every volume
  so the crawler covers the search. `--index-declined` puts the real client behind a launcher that
  starts nothing, which is how `tools/ui/index-degraded.bbs` photographs the degraded status bar
  without anyone having to decline a real prompt.
- **`AppPaths.OverrideVariable` (`BERTBROWSER_DATA_DIR`) is what keeps a run out of the user's
  data.** It is read by a static initialiser, so it must be set before anything touches `AppPaths`;
  `UiSession.Start` sets it and then *asserts* `AppPaths.DataDir` actually moved, refusing to run
  rather than indexing and deleting against the real database. Destructive commands are additionally
  fenced to the run's sandbox, since the harness drives the real delete and transfer executors.
- **A capture renders the window and crops, and is measured by the window's own `RenderSize`.** Two
  traps, both silent. Every window here draws its own title bar through `WindowChrome`, so the
  window's visual covers the whole frame while `Window.Content` sits below the caption and inside
  the root panel's margin — measuring the content and painting the window (which is what the
  equivalent tool for a native-caption app does) produced pictures 34 px short at the bottom, and
  32 px short again for any dialog whose root panel had a margin. And a child element must *not* be
  re-hosted through a `VisualBrush` to get it to the origin, tempting as that is: **WPF caches a
  `VisualBrush`'s realisation, and a `SolidColorBrush` inside it changing colour does not invalidate
  that cache.** One capture taken before a `theme` command made every capture after it come back in
  the old theme's colours, while the brushes, the resources and the elements' own properties all
  said the new theme had applied — a convincing-looking theming bug that was entirely in the
  harness. `Capture.CropTo` renders the root and cuts the element out of it; there is no cache to go
  stale. `probe <token> [element]` is the command that settled it, and is worth reaching for again:
  it prints what the resolver produced, what the app and window resources hand back, and what the
  element's own `Background`/`Foreground` are.
- **Dialogs are shown modelessly and photographed, never `ShowDialog`n.** `ShowDialog` runs a nested
  message loop on the script's own thread, so the run would hang until the watchdog fired. Each has
  an `internal static Create` beside its public `Show`, going through the same constructor, so a
  capture cannot drift from what the app puts on screen.

Measured, not assumed: **the window reaches the foreground exactly once**, during the first layout
pass, through no event the process is told about — `ShowActivated=false`, `WS_EX_NOACTIVATE`,
disabling the window and refusing WPF focus all fail to prevent it. `ForegroundGuard` polls at 10 ms
and hands it straight back; the count is in `state`, and more than one means something new is
activating the window. Quiescence is `FileListViewModel.IsLoading` across **every** tab plus
`ShellViewModel.IsTransferring`, pumped at `DispatcherPriority.Background` (which is what lets the
continuations run) and finished with one `ContextIdle` pass; a search additionally needs the 200 ms
debounce waited out, which is what `SettleSearch` is. `MainWindow.Loaded` is where
`InitializeAsync` starts, so a settle straight after `Show()` finds nothing loading and returns
immediately — `WaitForFirstListing` waits for a tab with a path in it instead.
