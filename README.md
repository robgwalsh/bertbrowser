# bertbrowser

[![Latest release](https://img.shields.io/github/v/release/robgwalsh/bertbrowser?label=release&color=1f6feb)](https://github.com/robgwalsh/bertbrowser/releases/latest)

An offline Windows 10/11 file browser built for my personal preferences.

<img src="docs/images/main_screenshot.png" alt="BertBrowser main window" width="500">

- **Offline** - BertBrowser does not connect to the Internet except a startup check against [GitHub Releases](https://github.com/robgwalsh/bertbrowser/releases) for app updates.
- **[Fast global search](docs/search-indexing.md)** - MFT indexing and USN journal tracking for fastest possible performance.
- **Directory sizes** — Show total size on directories, just like files.
- **Split panes with tabs** - Infinite pane splitting and tabs per pane.
- **Themes** - Rich theming system, with many pre-loaded themes.

## Install

```powershell
winget install RobWalsh.BertBrowser
```

* **Or** grab the [latest installer](https://github.com/robgwalsh/bertbrowser/releases/latest/download/BertBrowser-win-Setup.exe) and run it.
* **Or** download the [latest portable executable](https://github.com/robgwalsh/bertbrowser/releases/latest/download/BertBrowser-win-Portable.zip) if you'd rather not install anything.

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

See [docs/build-and-release.md](docs/build-and-release.md) for packaging, the tag-driven release workflow, and how updates reach installed copies.

## Testing the interface

`dotnet test` covers everything in Core — the planners, the executors, path keys, themes, layout.
What it cannot cover is the window itself, and running the app to look at it means putting a window
over whatever you were doing and losing your keyboard to it.

`tools/BertBrowser.Harness` is the answer: it builds the app's own service graph and shows the real
`MainWindow` parked outside every monitor and refused activation, then drives it with a small script
language and captures it with `RenderTargetBitmap` — a software re-render of the visual tree, so
being offscreen and covered costs nothing.

```powershell
$harness = "tools\BertBrowser.Harness\bin\Debug\net10.0-windows\BertBrowser.Harness.exe"

& $harness --script tools\ui\smoke.bbs      # browse, search, move, rename, delete, undo
& $harness --script tools\ui\themes.bbs     # every built-in theme, and the dialogs
& $harness -c "tree .; refresh; shot look"  # ad hoc; prints the PNG path
& $harness --help
```

Each run gets a throwaway fixture tree and its own scratch `BERTBROWSER_DATA_DIR`, so it never
touches your real index, settings or themes. It starts no programs, never touches the clipboard, and
refuses to write outside its sandbox — the harness drives the real transfer, rename and delete
executors, not stubs.

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
- `tools/BertBrowser.Harness` — hosts the real window offscreen and scripts it; `tools/ui/*.bbs` are the scripts.

See [CLAUDE.md](CLAUDE.md) for a deeper architecture walkthrough (path-key invariants, migrations, the size-scan algorithm).

## License

[MIT](LICENSE)
