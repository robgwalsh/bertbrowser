# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

bertbrowser is a Windows-only WPF file browser (net8.0-windows) with global MFT-backed search and cached recursive directory sizes, backed by a local SQLite database.

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

- `src/BertBrowser.Core` — plain net8.0, no UI dependencies: SQLite persistence, path canonicalization, search/size services. This is the only project with tests; keep anything testable here rather than in the App.
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

`DirectorySizeService` does an iterative post-order DFS so one scan caches results for the root **and every descendant** directory (`dir_size_cache`). It skips reparse points (junctions/symlinks) to avoid cycles and double-counting, flags results `incomplete` on access-denied instead of failing, limits concurrent scans to 2, and on cancellation writes nothing (cache keeps prior values).

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
`ShellViewModel.RetireUndoableTransfer` is what finally commits a replacement, so staged data
outlives the undo record by exactly one operation. Copy has no Replace and no undo: it is defined
as purely additive.

Tests are the point of this design — `TransferPlannerTests` (rules, fake filesystem),
`TransferExecutorTests` (real files, contents asserted), and `TransferRoundTripTests` (property
tests: the multiset of file contents under the root is invariant under any move, and undo restores
the tree byte-for-byte). Two meta-tests assert those invariant checks can actually detect a lost or
moved file. If you change this code, mutate a rule and confirm a test goes red.

### App shell

`ShellViewModel` is the root VM composing `FileListViewModel`, `FolderTreeViewModel`, and `BookmarksViewModel`, and owns navigation (back/forward stacks, breadcrumbs) with a `CancellationTokenSource` per navigation. The file list has two modes: normal directory listing, or — while the search box has a query — a flattened result list (`FileListViewModel.IsFlattened`) that also shows each hit's folder relative to the search root.

The file list is a multi-select (`Extended`) `ListView`. Ctrl/Shift/Ctrl+A come from WPF; `Views/MarqueeSelector` adds the rubber band — pressing on empty space and dragging sweeps a rectangle (drawn as an `Adorner`) that selects everything it touches, with edge auto-scroll. It only hit-tests *realized* containers, so it pins the drag origin to the row it landed on and recomputes the anchor from that row's live position, treating the anchor as off-screen once it virtualizes away. Selection-driven work (folder-tree reveal, status-bar summary) must therefore tolerate churn: the tree reveal is skipped while `MarqueeSelector.IsDragging`, and the summary is coalesced to one dispatcher pass. `PropertiesViewModel` takes a whole `IReadOnlyList<PropertiesTarget>` — with several targets it shows aggregates and three-state attribute checkboxes (indeterminate = the items disagree, and Apply leaves that bit alone).

`Views/FileDragDropController` drags that selection onto a folder row or a tree node to move it
(Ctrl to copy), using a private `BertBrowser.FileItems` data format so drops stay in-app. Two
details matter: pressing an already-selected row **defers** WPF's collapse-to-one-item selection to
mouse-up, or a multi-item drag would carry one item; and the plan computed while hovering is only
advisory — the drop always re-plans from scratch before writing.

The left sidebar has two sections: **Bookmarks** (top, sized to content) and **Drives & devices** (below, fills the rest). `FolderTreeViewModel.Roots` is `ObservableCollection<ISidebarNode>` mixing browsable `DirectoryNodeViewModel` drives (expandable tree) with `PortableDeviceNodeViewModel` leaves — MTP phones/cameras enumerated off-thread via `Interop.PortableDevices` (Shell.Application COM on an STA thread) that open in Explorer on double-click, since their contents aren't a filesystem path. Bookmarks persist in the `bookmark` table via `BookmarkRepository`/`IBookmarkService`; the file-list and tree context menus toggle them (`ShellViewModel.ToggleBookmarksAsync`), and `BookmarksViewModel` keeps an in-memory key set so the menu can label Bookmark/Remove without a DB hit.
