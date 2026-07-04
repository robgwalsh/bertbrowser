# bertbrowser

An offline Windows file browser built for my personal preferences. The overall design intent is **simplicity over customization**.

## What it does

- **Browse** — Expected modern file browser features
- **Search** - Windows search that doesn't blow
- **Tag files** — Express multiple organizational views beyond the default hierarchical viewpoint of a file browser
- **Directory sizes** — Show total size on directories, just like files

## What it does not do

- **Connect to the Internet** — except a startup check against [GitHub Releases](https://github.com/robgwalsh/bertbrowser/releases) for app updates

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
| Tag + size-cache database | `%USERPROFILE%\.bertbrowser\bertbrowser.db` |
| Window/session settings | `%USERPROFILE%\.bertbrowser\settings.json` |

Delete the folder to reset the app completely.

## Project layout

- `src/BertBrowser.Core` — everything testable and UI-free: SQLite persistence and migrations, path canonicalization, tag and directory-size services.
- `src/BertBrowser.App` — the WPF shell (MVVM via CommunityToolkit.Mvvm, DI via Microsoft.Extensions.DependencyInjection).
- `tests/BertBrowser.Core.Tests` — xUnit tests for Core; they run against real temp SQLite databases and directory trees.

See [CLAUDE.md](CLAUDE.md) for a deeper architecture walkthrough (path-key invariants, migrations, the size-scan algorithm).

## License

[MIT](LICENSE)
