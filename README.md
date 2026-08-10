# bertbrowser

[![Latest release](https://img.shields.io/github/v/release/robgwalsh/bertbrowser?label=release&color=1f6feb)](https://github.com/robgwalsh/bertbrowser/releases/latest)

An offline Windows file browser built for my personal preferences.

- **Offline** - BertBrowser does not connect to the Internet except a startup check against [GitHub Releases](https://github.com/robgwalsh/bertbrowser/releases) for app updates.
- **Fast global search** - MFT indexing and USN journal tracking for fastest possible performance
- **Directory sizes** — Show total size on directories, just like files.
- **Fast media thumbnails** - Show thumbnails for media files without the sluggishness of Windows Explorer. Control thumbnail size and aspect ratio.
- **Split panes with tabs** - Infinite pane splitting and tabs per pane
- **Themes** - Rich theming system, with many pre-loaded themes.

## Install

No .NET or other prerequisites — the installer is self-contained (Windows 10/11, x64). It installs per-user (no admin prompt), and the app keeps itself up to date automatically.

**winget:**

```powershell
winget install RobWalsh.BertBrowser
```

**Or directly:** grab `BertBrowser-win-Setup.exe` from the [latest release](https://github.com/robgwalsh/bertbrowser/releases/latest) and run it. There's also a `BertBrowser-win-Portable.zip` if you'd rather not install anything.

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
| Size-cache + search-index database | `%USERPROFILE%\.bertbrowser\bertbrowser.db` |
| Window/session settings | `%USERPROFILE%\.bertbrowser\settings.json` |

Delete the folder to reset the app completely.

## Project layout

- `src/BertBrowser.Core` — everything testable and UI-free: SQLite persistence and migrations, path canonicalization, search-index and directory-size services.
- `src/BertBrowser.App` — the WPF shell (MVVM via CommunityToolkit.Mvvm, DI via Microsoft.Extensions.DependencyInjection).
- `tests/BertBrowser.Core.Tests` — xUnit tests for Core; they run against real temp SQLite databases and directory trees.

See [CLAUDE.md](CLAUDE.md) for a deeper architecture walkthrough (path-key invariants, migrations, the size-scan algorithm).

## License

[MIT](LICENSE)
