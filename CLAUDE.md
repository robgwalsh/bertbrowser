# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

bertbrowser is a Windows-only WPF file browser (net8.0-windows) with file tagging and cached recursive directory sizes, backed by a local SQLite database.

## Commands

```powershell
dotnet build bertbrowser.sln          # build everything
dotnet test bertbrowser.sln           # run all tests (xUnit, Core only)
dotnet test tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests"   # one test class
dotnet test tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests.MethodName"  # one test
dotnet run --project src/BertBrowser.App   # launch the app (optional arg: start directory)
```

`Directory.Build.props` sets `TreatWarningsAsErrors` and `Nullable` for all projects — any warning fails the build.

## Structure

- `src/BertBrowser.Core` — plain net8.0, no UI dependencies: SQLite persistence, path canonicalization, tag/size services. This is the only project with tests; keep anything testable here rather than in the App.
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

`Db` (Core/Data) is the connection factory and migration runner. Connections open in WAL mode with foreign keys on. Migrations are embedded resources at `Data/Migrations/NNN_*.sql`, applied in order and tracked via `PRAGMA user_version`. To change the schema, add a new numbered `.sql` file — the csproj glob picks it up; never edit an existing migration. The live DB is `%LOCALAPPDATA%\BertBrowser\bertbrowser.db` (settings: `settings.json` in the same folder).

Repositories (`TagRepository`, `DirSizeRepository`) are synchronous ADO.NET; `TagService` is the async facade that keeps ViewModels off the SQLite calls. Follow that layering for new data access.

### Directory sizes

`DirectorySizeService` does an iterative post-order DFS so one scan caches results for the root **and every descendant** directory (`dir_size_cache`). It skips reparse points (junctions/symlinks) to avoid cycles and double-counting, flags results `incomplete` on access-denied instead of failing, limits concurrent scans to 2, and on cancellation writes nothing (cache keeps prior values).

### App shell

`ShellViewModel` is the root VM composing `FileListViewModel`, `FolderTreeViewModel`, and `TagFilterViewModel`, and owns navigation (back/forward stacks, breadcrumbs) with a `CancellationTokenSource` per navigation. The file list has two modes: normal directory listing, or — when the tag filter is active — a flattened list of all tagged files under the current subtree. After tag edits, call `ShellViewModel.OnTagsChangedAsync()` so chips and counts refresh.
