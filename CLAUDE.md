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
  a pipe, and **outlives it** (see the gotcha below). One of two elevated helpers.
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
| Queue / pause / resume | `Core/Services/Transfer/PauseGate`, `TransferQueueRules`, `ViewModels/TransferQueueViewModel`, `ShellViewModel.EnqueueAsync`/`DrainQueueAsync`, `Views/TransferProgressWindow` |
| Conflict resolution (per item) | `Core/Services/Transfer/IConflictPrompt`, `ViewModels/TransferConflictsViewModel`, `Views/TransferConflictDialog`, `Views/ConflictPrompt` |
| Merging a folder into a folder | `Core/Services/Transfer/TransferMergeExpander` (+ `TransferMergeLimits`), `ITransferEntrySource`, `TransferClash`/`ConflictDefaults` in `TransferModels`, `ShellViewModel.ExecuteDropAsync` |
| Right-drag verb menu | `Core/Services/Transfer/RightDropMenuRules`, `Views/DropPipeline.BuildVerbMenu`, `Views/DragSession` |
| Shortcuts (`.lnk`) | `Core/Services/Shortcuts/*` (`ShortcutPlanner`, `ShortcutExecutor`, `IShortcutWriter`), `Interop/ShellLink` |
| Rename (incl. advanced/tokens) | `Core/Services/Rename/*` (`RenamePattern`, `RenamePlanner`, `RenameExecutor`, `RenameRule`) |
| Create new item / ShellNew | `Core/Services/NewItem/*`, `Interop/ShellNewRegistry` |
| Delete / Recycle Bin | `Core/Services/Delete/*` (incl. `ShellRecycleBin`) |
| Disk usage | `Core/Services/DiskUsage/*`, `Views/DiskUsageWindow`, `TreemapLayout` |
| Duplicate finder | `Core/Services/Duplicates/*` |
| Compare / sync two folders | `Core/Services/Compare/*`, `ViewModels/CompareSessionViewModel` |
| Compare two files by content | `Core/Services/Compare/FileContentComparer`, `FileComparePlan`, `ContentSettlement`, `Core/Services/Diff/*` (`TextDiffer`), `Views/FileCompareWindow` |
| Checksums (hash / verify) | `Core/Services/Checksums/*` (`ChecksumAlgorithms`, `DigestSink`, `IFileDigester`, `ChecksumFile`, `ChecksumPath`, `ChecksumVerify`, `ChecksumRunner`), `Views/ChecksumWindow` |
| Search query language | `Core/Services/Search/*`, `docs/search-indexing.md` |
| Content search (`content:`) | `Core/Services/Search/ContentTerm.cs`, `Core/Services/Search/ContentReader.cs` |
| Saved searches | `Core/Services/SavedSearches/*` (`SavedSearchRules`), `Core/Data/SavedSearchRepository`, `ViewModels/SavedSearchesViewModel`, `Views/SavedSearchDialog` |
| Elevated MFT indexer | `src/BertBrowser.Indexer`, `Core/Services/Mft/MftIndexClient`, `Core/Ipc/IndexEndpoint`, `Core/Ipc/IndexerPresenceLock` |
| Indexer banner / sign-in task | `Core/Services/Mft/IndexerBannerRules`, `IndexerAutoStartTask`, `App/Services/Indexing/IndexAutoStartService` |
| Change timeline ("What changed") | `Core/Services/Changes/*` (`ChangeLogRules`, `ChangeRecorder`, `ChangeLogPolicy`), `Core/Data/ChangeLogRepository`, `Views/ChangeTimelineWindow`, the History page of `SettingsWindow` |
| Elevated file-op retry | `src/BertBrowser.Elevator`, `Core/Services/Elevation/*`, `Core/Ipc/ElevationProtocol.cs` |
| Launching other programs | `App/Services/ProcessLauncher.cs`, `Core/Services/ExecutablePath.cs`, `Core/Services/VSCodePath.cs`, `Interop/RunAsVerbRegistry` |
| Shell context menu (7-Zip, Git, TortoiseSVN…) | `Core/Services/ShellMenu/*` (`ShellMenuKeys`, `ShellMenuRules`, `StaticVerbCommand`), `Interop/ShellExtensionRegistry`, `Interop/ShellContextMenu`, `Interop/ShellSelection`, `Interop/ShellMenuIcons`, `Services/ShellMenuSource`, `Views/ShellMenu`, `BuiltInMenuItems` + `MenuSeparatorRules` + `Views/BuiltInMenu` (the app's own entries, unticked the same way), the Context menu page of `SettingsWindow`, `tools/ui/shellmenu.bbs` |
| Startup / CLI / single instance | `Core/Cli/CommandLine.cs`, `Core/Cli/NavigationRequest.cs`, `Services/SingleInstance.cs`, `Core/Ipc/InstanceEndpoint.cs`, `Interop/ForegroundWindow`, `Core/Services/Foreground/ForegroundRaiseRules` |
| Default folder handler (shell) | `Core/Services/ShellIntegration/*`, `App/Interop/FolderHandlerRegistry` |
| Preview pane (incl. hex/raw) | `Core/Services/Preview/*` (`PreviewClassifier`, `TextPreviewReader`, `HexPreviewReader`, `SyntaxTokenizer`) |
| Archives (zip/7z/tar/rar) | `Core/Services/Archives/*` (`ArchivePath`, `ArchiveReader`, `ArchiveIndexBuilder`) |
| Theming | `Core/Theming/*` (`ThemeCatalog`, `ThemeResolver`), `App/Theming/*` |
| Matching the Windows theme | `Core/Theming/SystemThemeRules`, `SystemAppearance`, `App/Theming/ISystemAppearance` |
| App icon | `tools/icon/build-app-icon.ps1` → `src/BertBrowser.App/Assets/app.ico` |
| Icons | `tools/icon/icons.txt` (the mapping) → `Resources/Icons.xaml` (generated), `IconPath`/`MenuIconPath`/`IconContent` in `Styles.xaml`, `tools/icon/IconSheet` |
| Columns (file list) | `Core/Services/Columns/*` (`ColumnCatalog`, `ColumnLayoutRules`, `ColumnCandidates`), `Interop/ShellProperties`, `Views/ColumnAddPanel` |
| Tabs / panes / layout | `App/ViewModels/DirectoryTabViewModel`, `PaneViewModel`, `ShellViewModel`, `Core/Layout/LayoutTree.cs` |
| Flat branch view (Ctrl+B) | `Core/Services/FlatView/FlatViewRules`, `SearchService.ListSubtreeAsync`, `DirectoryTabViewModel.FlatView`, `FileListViewModel.IsFlatBrowse` |
| Thumbnail tiles / scrolling | `Views/VirtualizingWrapPanel`, `ThumbnailTemplateSelector`, `ThumbPanel`/`ThumbTileTemplate` in `Styles.xaml`, `tools/ui/tiles.bbs` |
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
- **The index helper outlives the app.** Closing a window ends a *session*, not the helper — which
  is why reopening costs no UAC prompt and no rebuild. Consequences that are easy to undo by
  accident: `MftIndexClient.Dispose` must **not** send `Shutdown` (only `Stop` may); the pipe name
  is well-known per user (`IndexEndpoint`) rather than nonced, because a helper started under a
  previous app has to find the next one; every session **replays** `Building`/`Complete` before
  `Ready`, since `IndexRefreshed` already fired for volumes finished before this app existed; and
  one helper per user is enforced by a mutex (`IndexerPresenceLock`) because two would tail one
  journal into one database. The app never launches one at startup — it attaches, and a banner
  offers the prompt (`IndexerBannerRules`). Nothing supervises the helper any more, so it stops
  itself when the app's schema, its own executable, or `BertBrowser.exe` beside it changes.
- **The change log (`fs_change`) is off by default and written only by the index helper's USN
  tail**, under a `ChangeLogPolicy` the app pushes over the pipe (`IndexVerb.Record`, one integer,
  never a path). Nothing reads the journal on demand; recording starts once a volume's build
  completes; rows carry the *record's* timestamp. The recorder must exclude the data directory and
  an empty flush must never touch the DB, or every DB write logs itself for ever. Turning the
  setting off wipes the table on both sides. **Recording continues while the app is closed**, since
  the helper keeps the last policy pushed — deliberate, so the timeline has no holes, and stated on
  the settings page. Any feature that keeps history follows the same rule: off until the user turns
  it on, on its own Settings page, with a way to clear it.
- **One `Process.Start` in the whole app**, in `ProcessLauncher`. A second call site is a bug. (The
  rule is about the App; the Indexer registers its sign-in task through Task Scheduler's COM API,
  which starts no process at all. The other carve-out is `IContextMenu::InvokeCommand` in
  `ShellContextMenuHandler`: what 7-Zip starts when clicked is 7-Zip's code, not ours. Static
  registry verbs — "Git Bash Here" — still go through `ProcessLauncher`.)
- **The shell context menu is composed per extension, never asked of the shell whole.** The
  app enumerates `shellex\ContextMenuHandlers` and `shell\<verb>` under the families
  `ShellMenuKeys` picks, then hosts each COM handler in its *own* `HMENU` and mirrors that into
  WPF `MenuItem`s (`Views/ShellMenu`). `CDefFolderMenu` would hand back Cut/Copy/Delete/Rename/
  Properties/Send to/Share alongside the extensions with no way to tell whose item is whose — and
  the per-extension checklist on the Context menu page needs exactly that attribution. Consequences
  worth keeping: the menu is WPF, so the harness photographs it detached like every other and a
  native `TrackPopupMenuEx` is never involved; `WM_INITMENUPOPUP` is delivered by hand through
  `IContextMenu2/3::HandleMenuMsg` before each submenu is walked, since no menu is ever up for
  Windows to send it; a session (`ShellMenuSession`) owns the COM objects, PIDLs and `HMENU`s and is
  released **after** `ContextMenu.Closed`, deferred via the dispatcher, because the `Click` that
  calls `InvokeCommand` has to have run first — and only that session, since a right-click may have
  opened the next; `Imaging.CreateBitmapSourceFromHBitmap` drops the alpha of a handler's PARGB32
  menu bitmap, so `ShellMenuIcons` reads the pixels with `GetDIBits` into `Pbgra32`; everything
  runs on the UI thread because the extensions are apartment-threaded; and it is never offered
  inside an archive or over a search result's empty space (no path, no folder). Everything is
  shown unless unticked, so a newly installed extension appears without a visit to Settings, and
  `HiddenAfterSave` keeps an id hidden while its extension is uninstalled. The harness swaps in
  `CannedShellMenuSource`, so a scripted run loads no foreign code and launches nothing. The app's
  own entries are unticked the same way: a `MenuItem` with `Tag="id"` in XAML is one row of
  `BuiltInMenuItems` (an untagged id throws rather than quietly showing), `BuiltInMenu.Apply` runs
  *first* and the two items a right-click hides for its own reasons go through `BuiltInMenu.Show`
  so both answers count, and `TidySeparators` runs *last* over the finished menu — the separators
  are declared between groups in XAML, so a hidden group would otherwise leave two touching.
- **The single-instance hand-off defeats the foreground lock on purpose, so it must ask before
  using it.** `ForegroundWindow.Raise` only takes the foreground when `ForegroundRaiseRules` says
  nothing is full screen; otherwise it flashes the taskbar button and does not even un-minimize.
  The reason it matters is that the hand-off is not a user gesture: the app is the registered
  handler for `Directory` and `Drive`, so *anything* that shell-opens a folder starts a second copy
  which grants its foreground rights over — and the running copy landed on top of full-screen video,
  with no pattern the user could see, because the trigger belonged to another process.
- **The app is `asInvoker`.** Only the two elevated helper exes (Indexer, Elevator) touch an
  administrator token. Don't reintroduce `requireAdministrator` on the app to fix an access-denied
  error — that's now expected behavior (a folder the app can't read, Explorer can't either).
- **Every executor (transfer/rename/delete/new-item/archive-edit) follows the same shape**: a pure
  `*Planner` deciding through a probe interface (testable without disk), an `*Executor` that
  re-applies the plan's rules against live disk state before writing, and "one item's failure never
  affects the others." Nothing ever does `Directory.Delete(recursive: true)` — use
  `Core/Services/DirectoryRemoval` (handles junctions correctly).
- **The right-drag verb menu is for our own drags only.** A foreign drop has to report an effect
  back before `Drop` returns and cannot wait on a menu, so an external right-drag keeps
  `DropInContract`'s answer (Explorer's own menu would need `TrackPopupMenuEx`). Two consequences
  that are easy to undo: `DragSession.IsRightButton` is remembered from the *press*, because by the
  drop the button is generally already up and `KeyStates` would say "left"; and the menu is
  **posted**, not opened inside `Drop` — `DoDragDrop`'s modal loop still owns the mouse there, so a
  menu opened under it never sees the click meant to choose from it.
- **The queue holds `IsTransferring` for its whole drain, and pause lives in `ProgressCoalescer`.**
  That flag is not "a transfer is running" but "this app is writing" — ten operations raise it, and
  the four long ones (drop/paste, extract, compress, archive-edit) now queue behind it instead of
  being dropped on the floor. Releasing it between jobs would let a rename slip into the gap and
  take the undo slot out from under a queue the user is watching, and would make the harness call a
  six-job drain finished five times over. Pausing is a `PauseGate` waited on in `FileProgress`,
  `BeginItem` and `BeginFile` — not in `IFileCopier`: all four executors already funnel their bytes
  through one coalescer, and `CopyFileExW` invokes its progress routine synchronously, so a progress
  delegate that blocks *is* a pause in the middle of a single large file. Consequences worth keeping:
  `PauseGate.Wait` returns false rather than throwing (an exception would unwind across the P/Invoke
  boundary out of a callback Windows is still inside), `ProgressCoalescer.Silent()` takes no gate
  because staging must finish once started, a paused run holds a half-written destination open so
  every other write stays blocked, `UiSession.Settle` therefore treats a paused queue as quiescent,
  and `TransferProgressViewModel` stops its stopwatch — `TransferRate` is fed elapsed time, so a
  watch left running turns a five-minute pause into an invented stall.
- **Only the conflict *dialog* was ever single-answer.** `TransferExecutor.Execute`, `ElevatedRetry`,
  the elevation IPC and `SyncPlanner` have all taken a per-path
  `IReadOnlyDictionary<string, ConflictResolution>` since they were written. Asking goes through
  `IConflictPrompt` (a seam, like `IUserConfirm`) inside `ExecuteDropAsync`, so a drop and a paste
  reach one dialog — paste used to pass `null` and quietly number the newcomer. `null` now means
  *ask*; pass an empty map to mean "nothing needs an answer". A clash is not always with disk:
  `TransferPlan.EarlierClaimantOf` names the other incoming item, because probing disk for a name
  nothing has written yet answers "missing".
- **A folder landing on a folder of the same name is merged, and the folder itself is never asked
  about.** `TransferMergeExpander` replaces that one item with the descendants that really clash,
  each named by its path relative to the drop; everything else is carried across without a question.
  It runs **once, at the drop, off the UI thread** — never inside `TransferPlanner.Plan`, which
  `DropPipeline.IsAllowed` calls on every drag-over, and which keeps taking the narrower
  `ITransferProbe` so it *cannot* enumerate. The rest is consequence: every emitted destination's
  parent already exists because the walk descends only where **both** sides have a real directory
  (the same property `SyncPlanner` keeps by acting on a folder whole or not at all), so nothing ever
  invents a folder nobody selected; **a link on either side is always a leaf**, because a junction at
  the *destination* pointing back into the source would merge a folder into itself by a path
  `Revalidate` never inspects — it only compares against `DestinationDirectory`, and an expanded
  source is always deeper; a move prunes the source folders it emptied (`plan.PruneDirectories`,
  non-recursive `Directory.Delete` only, never a link) and `Undo` recreates exactly those before
  restoring, so the "its original folder is gone" guard keeps meaning *the user* deleted it; the
  clash's two sides are carried on the plan (`TransferClash`) so a dialog of five thousand rows does
  no disk IO; "identical" is `CompareEquality.CompareTimes` at **`Strict`** tolerance, never `Loose`,
  since forgiving an hour here pre-selects Skip on a file that really is an hour newer; and past
  `TransferMergeLimits` the folder keeps its old wholesale question rather than becoming a dialog
  nobody can answer.
- **There is one undo slot**, shared across move/rename/delete/archive-edit/sync — five-way, one
  level, whichever operation happened last. `RetireUndoable` is what finally commits staged/held
  data — call it before assuming a Replace or Delete's staging is irreversibly gone. Sync is the
  only arm that is two operations at once (copies *and* removals). **A copy's outcome still reports
  `CanUndo == false`** — that property means "`Undo()` will work", and `Undo` refuses a copy —
  and `ConflictResolution.Overwrite` exists precisely because a copy that displaced something with
  no record kept would strand it in staging for ever. Only a caller that keeps the outcome may ask
  for it, and there are now two: a sync, and a **merged** drop or paste, which says so through the
  separate `CanUndoCopy` and is reversed by `UndoCopies`. That one must never be routed through
  `ElevateIfRefusedAsync` — `ElevationHost.UndoTransfer` hardcodes `TransferVerb.Move`, so handed a
  copy's outcome it would move the pasted files back into the folder they came from instead of
  removing them, corrupting both sides.
- **A comparison's "same" is what authorises a delete**, so every doubt resolves away from it: a
  missing timestamp is `Unknown` and one `Unknown` descendant carries a whole subtree to `Unknown`.
  `dir_size_cache` deliberately never classifies a folder — equal totals do not mean equal trees,
  and the rows are missing on exactly the unmeasured backup drive the comparison is usually about.
- **One opener, `Core/Services/ReadOnlyFile`.** The share flags, the cloud-placeholder refusal and
  the reparse-point rule now have three callers — the hasher, the digester and the content
  comparison — and a third copy of them is a third chance for one to drift, in a way that fails
  silently both ways (too narrow and the app fights its own rename; too wide and a preview starts a
  multi-gigabyte download).
- **Checksums: two seams over one read loop, and the CRC-32 byte order.** `IFileHasher` stays
  SHA-256-only because the duplicate finder's safety argument is that answer authorises a delete;
  `IFileDigester` is the user-chosen one. `FileSystemFileHasher` implements both over one loop, so
  `FileSystemFileHasherTests` being unchanged is the proof the refactor was invisible.
  `System.IO.Hashing.Crc32` returns its checksum **little-endian** and every `.sfv` states it
  big-endian — the reversal in `Crc32DigestSink` is guarded by a known-vector test because getting
  it wrong yields a plausible digest nothing else would catch. Digests are uppercase everywhere
  inside Core; casing is decided in exactly one place (`ChecksumAlgorithms.IsWrittenLowercase`) and
  applied only when rendering.
- **A checksum file is untrusted input.** Every name in one is about to become a path this app
  opens, so `ChecksumPath.Resolve` refuses anything rooted, device-prefixed or escaping the folder,
  and returns null rather than throwing — a hostile line becomes a visible row in the report. The
  format's line order comes from the *extension*, never a sniff: `.sfv` is `name CRC`, the `*sum`
  family is `digest  name`, and guessing would make a legitimate file unreadable in a way its
  author could never diagnose.
- **Content settlement is one-directional, earned, and rewrites the `CompareResult`.** Identical
  bytes may raise `LeftNewer`/`RightNewer`/`Unknown` to `Same`; nothing else moves, and
  `Unreadable` is never evidence. It produces a **new** `CompareResult` rather than an overlay on
  the session, because `SyncPlanner` reads verdicts straight off the result — an overlay would
  paint a row green while the sync went on copying it. Roll-ups are re-folded from scratch, since
  `CompareRules.RollUp` only ever raises rank. A leaf `Differs` is unsettleable in practice: it
  means equal timestamps and *unequal sizes*.
- **`SearchNode.Matches` (definition) and `WriteSql` (optimization) must never disagree** — SQL may
  be a superset (re-checked per row) but never a subset. `ContentTerm` extends this to three-valued
  matching (`Yes`/`No`/`NeedsContent`) since content can't be answered from a column.
- **`IsFlattened` says the rows come from many folders; `IsFlatBrowse` says they still have one
  home.** A search sets the first alone, Ctrl+B sets both, and which one a guard reads is the whole
  point. The Folder column, the folders-first sort band, the watcher merge and a folder comparison
  follow `IsFlattened` — a one-level watcher listing genuinely cannot be merged into a recursive
  one, so a flat view has no live refresh and F5 re-runs it. New, Compress and a drop on empty space
  follow the narrower `IsSearchResult`, because a flat browse has a "here" and a whole-PC search does
  not. Set the flag *before* `BeginFlattened`, since a comparison ends on `IsFlattened` changing and
  words its message from it. Flat is deliberately **not** cleared by navigation, unlike search: that
  is what makes it a mode rather than a gesture, and it is what every other Ctrl+B does. It reads the
  disk and never the index — a browse surface must be true rather than instant, the line
  `FolderCompareService.UsesIndex` drew first — while the estimate behind the are-you-sure prompt
  does come from `dir_size_cache`, at one primary-key lookup, because an estimate only decides
  whether to ask a question.
- **The thumbnail view's panel must be a virtualizing one, and it is ours.** It was a `WrapPanel`,
  which is not a `VirtualizingPanel`, so the list built a container for every row — and every
  container asks for a thumbnail as soon as it is realized. Measured: a flat view of 1,000 videos
  took 3.9 s as a details list and never finished inside a four-minute watchdog as tiles; the panel
  alone, with the thumbnail requests taken out, still cost 6 s a thousand rows and grew worse than
  linearly, because every arriving thumbnail re-measured all of them. `VirtualizingWrapPanel` places
  items by arithmetic rather than by measuring them, which is why **tiles have to be uniform** —
  hence the fixed caption height in `ThumbTileTemplate` — and why `ThumbnailTemplateSelector.IsTile`
  is shared: the panel deciding an item's shape differently from the template would lay the grid out
  for one thing and fill it with another. `tools/ui/tiles.bbs` is what holds this down.
- **A thumbnail is a megabyte, and WPF recycles the container rather than the view model.** So a row
  keeps its decoded bitmap for the listing's whole life, and memory grows with everything ever
  scrolled past — invisible over a folder of forty photos, ruinous over a flat view of a media tree.
  Shell calls go through a four-slot gate and retention is trimmed against `RealizedRows` at
  `Background` priority, both copied from `ShellMetadataHydrator`, which got this right first. That
  narrowing looks for a `VirtualizingPanel`, not a `VirtualizingStackPanel`: naming the details
  list's exact panel type made it answer "nothing is on screen" for every tile ever shown, which
  turned the trim into a loop that released the very rows being looked at. It returns **null, not
  empty**, when there is no panel — before the first layout pass the answer is unknown rather than
  none, and the two are not the same question.
- **Nothing holds a file open** across previews, content search, or duplicate hashing —
  `FileShare.ReadWrite | Delete` everywhere, or this app's own rename/move/delete blocks itself.
  Cloud placeholders (`NotDownloaded`/`Offline`) are refused rather than silently hydrated (also
  true for shell-metadata columns — see `MetadataReadRules`).
- **Never launch the app to check UI work.** Use the `verify` skill / harness. `RenderTargetBitmap`
  re-renders the visual tree offscreen; posted input goes through `WM_KEYDOWN`/`WM_CHAR`, not
  `SendKeys`. Dialogs are shown modelessly and screenshotted, never `ShowDialog`. **`App.OnStartup`
  fires under the harness anyway** — WPF queues `Startup` from the `Application` constructor, so
  not calling `Run()` prevents nothing — and `App.UseServices` marking the app as hosted is what
  stops it. Before that guard, every run built a second real graph and showed a second main
  window; code-behind that resolves from `App.Services` gets the harness's graph only because of it.
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
- **Matching Windows is the default only because there is no settings file.** `FollowSystemTheme ??
  !LoadedFromDisk` — not `ThemeId is null`, which every existing user who never opened Settings also
  has, and flipping all of them on upgrade is exactly the surprise. `Initialize` pins the answer on
  first run, or launch two finds a file and reads the null as "no". A system-driven switch goes
  through `ApplyForCurrentState`, never `SelectTheme`, because `SelectTheme` writes `ThemeId` and
  the OS writing it would convert a following user into a pinned one at sunset; `ThemeId` is left
  untouched while following so unticking restores it. High contrast wins over both slots and never
  consults them, and the light/dark bit is kept *underneath* it so turning it off lands back on the
  right slot. A run poses all of this through `FakeSystemAppearance` and starts pinned, so screenshots
  do not depend on the developer's own desktop — which the old direct `SystemParameters.HighContrast`
  read did.
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
