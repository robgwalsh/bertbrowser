# CLAUDE.md

Guidance for Claude Code working in this repo.

## Project

bertbrowser is a Windows-only WPF file browser (net10.0-windows) with global MFT-backed search and
cached recursive directory sizes, backed by a local SQLite database.

## Commands

```powershell
dotnet build bertbrowser.sln
dotnet test bertbrowser.sln
dotnet test tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests"

# Drive the real window offscreen — see "Never launch the GUI" below.
tools/BertBrowser.Harness/bin/Debug/net10.0-windows/BertBrowser.Harness.exe --script tools/ui/smoke.bbs
```

`Directory.Build.props` sets `TreatWarningsAsErrors` and `Nullable` for all projects — any warning
fails the build. If a build fails with MSB3021/MSB3026 (a running instance locks `bin\Debug`), kill
it (`Get-Process BertBrowser | Stop-Process -Force`) and rebuild — no need to ask.

## Never launch the GUI

`dotnet run --project src\BertBrowser.App` and `BertBrowser.exe` put a real window on the user's
screen and steal focus. **Never run either**, even "just to check something." Use the harness
(`tools/BertBrowser.Harness`, driven via the `verify` skill) to see the interface offscreen. The
only exception is the user explicitly asking you to launch it for real.

## Structure

- `src/BertBrowser.Core` — plain net10.0, no UI deps: SQLite persistence, path canonicalization,
  search/size/transfer/rename/delete/etc. services. The only project with tests; put anything
  testable here rather than in the App.
- `src/BertBrowser.App` — WPF shell (MVVM via CommunityToolkit.Mvvm, DI via
  `Microsoft.Extensions.DependencyInjection`). Composition root: `App.BuildServices()` in
  `App.xaml.cs` (`App.Services`).
- `src/BertBrowser.Indexer` — elevated console exe hosting `MftIndexService`, talks to the app over
  a pipe. One of two elevated helpers.
- `src/BertBrowser.Elevator` — elevated console exe for a single retried file operation (move/copy
  /delete/rename/create refused for permissions), started on demand, serves one request, exits.
- `tests/BertBrowser.Core.Tests` — xUnit; creates real SQLite DBs and directory trees under `%TEMP%`.
- `tools/BertBrowser.Harness` — offscreen UI harness; `tools/ui/*.bbs` are its scripts.

## Where to look, by topic

Each area below is implemented as a `Core/Services/<Area>` (pure decide-logic + executor, unit
tested) plus thin App-side view models/views. Read the source for behavior — this file only tells
you where and what to watch for.

| Topic | Look at |
|---|---|
| Path canonicalization | `Core/Paths/PathKey.cs`, `Core/Paths/UniquePath.cs` |
| Database / migrations | `Core/Data/Db.cs`, `Core/Data/Migrations/NNN_*.sql` |
| Directory sizes | `Core/Services/MftDirectorySizeBuilder`, `DirSizeRepository`, `docs/search-indexing.md` |
| Move/copy/drag-drop/paste | `Core/Services/Transfer/*` (`TransferPlanner`, `TransferExecutor`, `IFileCopier`) |
| Rename (incl. advanced/tokens) | `Core/Services/Rename/*` (`RenamePattern`, `RenamePlanner`, `RenameExecutor`, `RenameRule`) |
| Create new item / ShellNew | `Core/Services/NewItem/*`, `Interop/ShellNewRegistry` |
| Delete / Recycle Bin | `Core/Services/Delete/*` (incl. `ShellRecycleBin`) |
| Disk usage | `Core/Services/DiskUsage/*`, `Views/DiskUsageWindow`, `TreemapLayout` |
| Duplicate finder | `Core/Services/Duplicates/*` |
| Compare / sync two folders | `Core/Services/Compare/*`, `ViewModels/CompareSessionViewModel` |
| Search query language | `Core/Services/Search/*`, `docs/search-indexing.md` |
| Content search (`content:`) | `Core/Services/Search/ContentTerm.cs`, `Core/Services/Search/ContentReader.cs` |
| Elevated MFT indexer | `src/BertBrowser.Indexer`, `Core/Services/MftIndexClient` |
| Elevated file-op retry | `src/BertBrowser.Elevator`, `Core/Services/Elevation/*`, `Core/Ipc/ElevationProtocol.cs` |
| Launching other programs | `App/Services/ProcessLauncher.cs`, `Core/Services/ExecutablePath.cs`, `Core/Services/VSCodePath.cs`, `Interop/RunAsVerbRegistry` |
| Startup / CLI / single instance | `Core/Cli/CommandLine.cs`, `Core/Cli/NavigationRequest.cs`, `Services/SingleInstance.cs`, `Core/Ipc/InstanceEndpoint.cs` |
| Default folder handler (shell) | `Core/Services/ShellIntegration/*`, `App/Interop/FolderHandlerRegistry` |
| Preview pane (incl. hex/raw) | `Core/Services/Preview/*` (`PreviewClassifier`, `TextPreviewReader`, `HexPreviewReader`, `SyntaxTokenizer`) |
| Archives (zip/7z/tar/rar) | `Core/Services/Archives/*` (`ArchivePath`, `ArchiveReader`, `ArchiveIndexBuilder`) |
| Theming | `Core/Theming/*` (`ThemeCatalog`, `ThemeResolver`), `App/Theming/*` |
| App icon | `tools/icon/build-app-icon.ps1` → `src/BertBrowser.App/Assets/app.ico` |
| Icons | `tools/icon/icons.txt` (the mapping) → `Resources/Icons.xaml` (generated), `IconPath`/`MenuIconPath`/`IconContent` in `Styles.xaml`, `tools/icon/IconSheet` |
| Columns (file list) | `Core/Services/Columns/*` (`ColumnCatalog`, `ColumnLayoutRules`, `ColumnCandidates`), `Interop/ShellProperties`, `Views/ColumnAddPanel` |
| Tabs / panes / layout | `App/ViewModels/DirectoryTabViewModel`, `PaneViewModel`, `ShellViewModel`, `Core/Layout/LayoutTree.cs` |
| UI test harness | `tools/BertBrowser.Harness`, `tools/ui/*.bbs`, `.claude/skills/verify` |

## Cross-cutting gotchas

- **Path keys**: every DB path column must store `PathKey.Canonicalize()` output (uppercased,
  no trailing separator except drive roots); use `PrefixBounds(dir)` for subtree range scans instead
  of `LIKE`. A row that isn't canonicalized breaks every subtree query silently.
- **Directory sizes are never computed on demand.** All numbers come from `dir_size_cache`, filled by
  the MFT pass. A missing row means *unknown* and must render blank, **never zero**.
- **Virtual (in-archive) paths must never reach a `PathKey`-keyed table.** `C:\x\a.zip\src` is a real
  Windows path syntactically, so it *would* land inside `PrefixBounds(C:\x)` — bookmarks, search and
  disk-usage each explicitly refuse a virtual root.
- **One `Process.Start` in the whole app**, in `ProcessLauncher`. A second call site is a bug.
- **The app is `asInvoker`.** Only the two elevated helper exes (Indexer, Elevator) touch an
  administrator token. Don't reintroduce `requireAdministrator` on the app to fix an access-denied
  error — that's now expected behavior (a folder the app can't read, Explorer can't either).
- **Every executor (transfer/rename/delete/new-item/archive-edit) follows the same shape**: a pure
  `*Planner` deciding through a probe interface (testable without disk), an `*Executor` that
  re-applies the plan's rules against live disk state before writing, and "one item's failure never
  affects the others." Nothing ever does `Directory.Delete(recursive: true)` — use
  `Core/Services/DirectoryRemoval` (handles junctions correctly).
- **There is one undo slot**, shared across move/rename/delete/archive-edit/sync — five-way, one
  level, whichever operation happened last. `RetireUndoable` is what finally commits staged/held
  data — call it before assuming a Replace or Delete's staging is irreversibly gone. Sync is the
  only arm that is two operations at once (copies *and* removals) and the only undoable copy: a
  copy's outcome still reports `CanUndo == false`, and `ConflictResolution.Overwrite` exists
  precisely because a copy that displaced something with no record kept would strand it in staging
  for ever. Only a caller that keeps the outcome may ask for it.
- **A comparison's "same" is what authorises a delete**, so every doubt resolves away from it: a
  missing timestamp is `Unknown` and one `Unknown` descendant carries a whole subtree to `Unknown`.
  `dir_size_cache` deliberately never classifies a folder — equal totals do not mean equal trees,
  and the rows are missing on exactly the unmeasured backup drive the comparison is usually about.
- **`SearchNode.Matches` (definition) and `WriteSql` (optimization) must never disagree** — SQL may
  be a superset (re-checked per row) but never a subset. `ContentTerm` extends this to three-valued
  matching (`Yes`/`No`/`NeedsContent`) since content can't be answered from a column.
- **Nothing holds a file open** across previews, content search, or duplicate hashing —
  `FileShare.ReadWrite | Delete` everywhere, or this app's own rename/move/delete blocks itself.
  Cloud placeholders (`NotDownloaded`/`Offline`) are refused rather than silently hydrated (also
  true for shell-metadata columns — see `MetadataReadRules`).
- **Never launch the app to check UI work.** Use the `verify` skill / harness. `RenderTargetBitmap`
  re-renders the visual tree offscreen; posted input goes through `WM_KEYDOWN`/`WM_CHAR`, not
  `SendKeys`. Dialogs are shown modelessly and screenshotted, never `ShowDialog`.
- **Icons are named, never numbered.** A wrong picture is the one UI mistake nothing catches: it
  compiles, renders, and passes every test. Call sites say `Data="{StaticResource Icon.Back}"`, so a
  bad name fails at load instead. Add one by editing `tools/icon/icons.txt` and re-running
  `pwsh tools/icon/build-icons.ps1` — never hand-edit the generated `Resources/Icons.xaml` — then
  **look at it**: `dotnet run --project tools/icon/IconSheet` sheets every icon with its name to a
  PNG (no window is shown). Drawn outlines from Fluent UI System Icons (MIT), not the Segoe font,
  which is Windows-11-only and not redistributable. What the codepoints cost before: `E76E` was a
  smiley face on the split-pane button, `E8B0` a mouse cursor on "Open in new pane", `E74B` a down
  arrow on "Delete permanently", and one number meant two things twice over (`E8A7` = new tab *and*
  custom command; `E8C8` = Copy *and* Find duplicates).
- **Theme colors**: no literal colors in XAML/C# — always a `Theme.*` token. `ThemeCatalogTests`
  contrast-checks every built-in; darken a palette's colors if it fails AA.
- **Never `Freeze()` anything holding a theme brush.** The `Theme.*` brushes are shared mutable
  instances — that is how a theme change recolours everything in place — so a `Pen`, `Drawing` or
  `GeometryDrawing` built over one is not freezable and `Freeze()` *throws* rather than merely
  pinning the colour. It has taken the app down once (`ListReorderDrag`, on the first mouse-move of
  a drag) after `MarqueeSelector` had already been fixed for it. Leave it unfrozen and it repaints
  on a theme change for free; freeze a `SolidColorBrush` you built yourself from a `ThemeColor`
  instead (`TreemapCanvas` does).
- **When editing a doc file with load-bearing prose** (this file, `docs/*.md`), keep new content as
  terse pointers/gotchas, not a re-narration of the code — the code and its tests are the source of
  truth; this file is a map, not a manual.
