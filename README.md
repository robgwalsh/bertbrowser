# bertbrowser

A Windows file browser built for my personal preferences. If it does what you want, great — but the design intent is **simplicity over customization**: there are no themes, no plugin system, and very few settings. Features get added when I need them, not when they can be toggled.

## What it does

- **Browse** — folder tree, file list, breadcrumbs, back/forward/up navigation. Double-click opens files with their default app. Natural sorting (`file2` before `file10`) and real shell icons.
- **Tag files** — create tags (with optional colors), assign them to files, and filter. The tag filter is subtree-aware: activate it and the file list flattens into every tagged file under the current folder, matching any or all of the checked tags.
- **Directory sizes** — compute the recursive size of any folder on demand. One scan caches results for the folder *and every subfolder* in SQLite, so drilling down afterwards is instant. Scans skip junctions/symlinks, survive access-denied folders (results are flagged incomplete rather than failing), and can be cancelled without losing previously cached values.

Everything is stored in a local SQLite database. Tags are attached to paths, not file contents — if you move a file outside the app, its tags don't follow (missing files can be pruned from the tag database in the UI).

## Requirements

- Windows 10/11
- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0) (to build; the app targets `net8.0-windows` / WPF)

## Building and running

```powershell
git clone https://github.com/robgwalsh/bertbrowser.git
cd bertbrowser

dotnet build bertbrowser.sln       # build
dotnet test bertbrowser.sln        # run tests
dotnet run --project src/BertBrowser.App              # launch
dotnet run --project src/BertBrowser.App -- C:\Some\Dir   # launch at a specific folder
```

Note that warnings are treated as errors across the solution (`Directory.Build.props`), so a clean build is a warning-free build.

## Data locations

| What | Where |
|---|---|
| Tag + size-cache database | `%LOCALAPPDATA%\BertBrowser\bertbrowser.db` |
| Window/session settings | `%LOCALAPPDATA%\BertBrowser\settings.json` |

Delete the folder to reset the app completely.

## Project layout

- `src/BertBrowser.Core` — everything testable and UI-free: SQLite persistence and migrations, path canonicalization, tag and directory-size services.
- `src/BertBrowser.App` — the WPF shell (MVVM via CommunityToolkit.Mvvm, DI via Microsoft.Extensions.DependencyInjection).
- `tests/BertBrowser.Core.Tests` — xUnit tests for Core; they run against real temp SQLite databases and directory trees.

See [CLAUDE.md](CLAUDE.md) for a deeper architecture walkthrough (path-key invariants, migrations, the size-scan algorithm).

## License

[MIT](LICENSE)
