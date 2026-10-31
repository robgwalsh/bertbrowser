# CLAUDE.md

Guidance for Claude Code working in this repo.

## Project

bertbrowser is a Windows-only file browser with global MFT-backed search and cached recursive
directory sizes, backed by a local SQLite database. The app is **native**: a Rust core (`crates`)
behind a C ABI, and a C++20 Win32 + Direct2D/DirectWrite UI (`app`), everything custom-drawn.
The earlier C#/WPF app lives on, frozen, in the private `bertbrowser-csharp-archive` repo as the reference for behaviour (see below).

## Commands

All from the repository root (PowerShell 7). Windows 10/11, Visual Studio 2022 with the C++ workload, Rust stable
`x86_64-pc-windows-msvc`, `cargo install cbindgen`.

```powershell
pwsh .\build.ps1 [-Config release] [-Configure]   # enters the VS dev shell; cargo + cbindgen + cmake. Output in out\<config>\
cargo test --workspace                             # run via PowerShell, NOT the Bash tool: its sandbox ignores Deny ACEs, so access_denied_tests fails there
cargo test -p bb-core --test all search_grammar        # one filter over the integration binary
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Drive the real window offscreen - see "Never launch the GUI" below. --out must be a fresh dir per run.
out\debug\harness\bb-harness.exe --script tools\ui\smoke.bbs --out $env:TEMP\look-1

# Measure - see bench/README.md. Release build.
target\release\bb-bench.exe run|ui|startup|compare|baseline|report|corpus   # --help for flags

pwsh tools\parity\tests.ps1 [-All] [-File X.cs]   # C# test vs Rust test, by file and name
```

`build.ps1` prints a harmless "vswhere.exe not recognized" line. `LNK1104` on `app\BertBrowser.exe`
means a running instance holds it - kill it (`Get-Process BertBrowser | Stop-Process -Force`) and
rebuild, no need to ask. `BERTBROWSER_DATA_DIR` overrides the data directory (`~\.bertbrowser`).
CI (`unstable.yml`, `release.yml`) builds only the app; the C# archive is a separate repo.

## Never launch the GUI

`BertBrowser.exe` puts a real window on the user's screen and steals focus. **Never run it**, even
"just to check something." Use the harness (`harness` -> `bb-harness.exe`, driven via the
`verify` skill). The only sanctioned launch is `bb-bench startup`, which parks the window offscreen
against a scratch data directory. Exception: the user explicitly asking you to launch it.

## Structure

- `src/crates/bb-paths` - `PathKey`, `UniquePath`, natural sort, glob text. No other deps.
- `src/crates/bb-core` - everything pure or Win32-behind-a-trait: SQLite (rusqlite, bundled),
  search, executors, undo, metadata codecs, archives, the settings model. **The place for anything
  testable**: tests are `src/crates/bb-core/tests/all/<module>/<name>_tests.rs` (`<module>` mirrors the `src` module
  under test; one binary, `main.rs` + a `mod.rs` per folder), real SQLite DBs and trees under `%TEMP%`.
- `src/crates/bb-shell` - shell/registry/COM services that are not painting (launcher, single
  instance, foreground, folder handler, updates, startup trace, index/elevation pipes).
- `src/crates/bb-app` - orchestration (tabs, listings, operations queue, palette, settings pages,
  tool-window models, session). No UI; the FFI, the harness and tests call its `api`.
  Folders: `api` (the public `impl App` surface), `browse`, `ops`, `tools`, `commands`, `side_pane`, `services`;
  the spine (`app`, `events`, `ids`, `seams`, `session`, ...) stays at the top.
- `src/crates/bb-ffi` - the C ABI staticlib `bbcore`; cbindgen reads this crate alone.
  `bb-ffi-harness` = the same ABI plus test fixtures (`bbcore_harness.lib`); `bb-test-support` = dev-dep fixtures.
- `src/crates/bb-indexer`, `bb-elevator` - the two elevated exes (`requireAdministrator`). They link
  `bb-core` + `bb-paths` only, never shell COM or orchestration. Indexer **outlives the app**
  (gotcha below); Elevator serves one retried file operation and exits.
- `src/crates/bb-bench` - benchmark driver.
- `src/app/{platform,gfx,ui,views,app}` - C++ static lib `bbapp` + `BertBrowser.exe` (WinMain only).
  `platform` = hwnd/loop/DPI/chrome; `gfx` = D3D11/D2D/DWrite, palette, text cache, icons; `ui` = widget
  tree and core widgets; `views` = main window, panes, dialogs, tool windows, shell-menu host.
  Both are grouped in subfolders, includes rooted at `src/app` (`"ui/popups/menu.h"`): `ui/{core,controls,collections,popups}`,
  `views/{bridge,main,chrome,panes,dialogs,tools,settings,shell}` (`bridge` = FFI + JSON models, the only layer on `bbcore.h`).
  A table entry below that names only a bare file (`ops_dialogs.cpp`) lives in one of those `views` folders.
- `harness` - `bb-harness.exe`: links `bbapp` + `bbcore_harness`, runs `tools/ui/*.bbs`.
- `tools/{icons,parity}`, `bench/`, `docs/`, `include/bbcore.h` (generated).
- `tools/ui/*.bbs` - the harness scripts, shared by the native harness and the C# one.
- The frozen C# reference is its own repo, `bertbrowser-csharp-archive` (below).

## C# reference (bertbrowser-csharp-archive)

The previous C#/WPF app (`src`, `tests`, `tools/BertBrowser.Harness`, `tools/BertBrowser.Bench`,
`bertbrowser.sln`, C# baselines) lives in the private repo `robgwalsh/bertbrowser-csharp-archive`, cloned beside this
one (`../bertbrowser-csharp-archive`, or `$env:BERTBROWSER_CSHARP_ARCHIVE`). It is **the spec, not shipped, and not built by CI**. When porting
or checking behaviour, the C# file is the specification: `Spec:` doc comments in the Rust and C++
point at it - read `Core/...` as `src/BertBrowser.Core/...` in the archive. Never edit it except to keep it
building. `tools/parity/tests.ps1` diffs the C# test suite against the Rust one (deliberate
drops in `tools/parity/dropped.txt`, with a reason).

## Where to look, by topic

Paths are Rust unless noted: `core/` = `src/crates/bb-core/src`, `app/` = `bb-app/src`, `shell/` =
`bb-shell/src`; `ui:` = C++ under `src/app/views` (or `ui`, `gfx`, `platform`). Read the source for
behavior - this table only says where.

| Topic | Look at |
|---|---|
| Path canonicalization | `bb-paths` (`key.rs` PathKey, `unique.rs`, `natural.rs`, `text.rs`) |
| Database / migrations | `core/data/db.rs`, `core/data/migrations/NNN_*.sql` (`001_schema.sql` is the fresh baseline) |
| Directory sizes | `core/mft/dir_size_builder.rs`, `core/data/dir_size.rs`, `docs/search-indexing.md` |
| Move/copy/drag-drop/paste | `core/transfer/*` (`planner`, `executor`, `copier`, `drop_in`, `drag_out`), `app/ops/mod.rs`, ui: `file_drag.cpp` (OLE drop source/target, clipboard) |
| Undo / redo history | `core/undo/*` (`stack`, `navigator`, `record` + `*_record`, `host`, `held_size`), `core/transfer/redo.rs`, `core/delete/redo.rs`, `app/ops/mod.rs`/`api/ops.rs`, ui: `ops_dialogs.cpp` (History window), `ops_menus.cpp` (title-bar split buttons), History page of `settings_view.cpp`, `tools/ui/undo-history.bbs` |
| Queue / pause / resume | `core/transfer/pause_gate.rs`, `queue_rules.rs`, `progress.rs`, `app/ops/mod.rs` + `ops/ui.rs`, ui: `ops_dialogs.cpp` (transfer window) |
| Conflict resolution (per item) | `core/transfer/conflict_prompt.rs`, `app/seams.rs`, `app/ops/ui.rs`, ui: `ops_dialogs.cpp` (conflicts dialog) |
| Merging a folder into a folder | `core/transfer/merge_expander.rs`, `models.rs` (`TransferClash`), `app/ops/mod.rs` |
| Right-drag verb menu | `core/transfer/right_drop_menu.rs`, `app/api/ops.rs`, ui: `file_drag.cpp`, `ops_menus.cpp` |
| Shortcuts (`.lnk`) | `core/shortcuts/*`, `app/ops/shell_link.rs` (the one writer, `IShellLink`) |
| Rename (incl. advanced/tokens) | `core/rename/*` (`pattern`, `planner`, `executor`, `rule`, `template`) |
| Create new item / ShellNew | `core/newitem/*` (`shell_new_import`), `app/ops/newitem.rs` |
| Delete / Recycle Bin | `core/delete/*` (`recycle_bin` = `IFileOperation` + restore over COM) |
| Disk usage | `core/diskusage/*`, `app/tools/disk.rs`, ui: `tool_dialogs.cpp`, `tool_widgets.cpp` (treemap) |
| Duplicate finder | `core/duplicates/*` |
| Compare / sync two folders | `core/compare/*`, `app/tools/compare_view.rs`, ui: `tool_banner.cpp` |
| Compare two files by content | `core/compare/{file_content_comparer,file_compare_plan,content_settlement}.rs`, `core/diff/*`, `app/tools/filecompare.rs` |
| Checksums (hash / verify) | `core/checksums/*`, `app/tools/checksum.rs`, ui: `tool_dialogs.cpp` |
| Metadata editing (pictures, audio/video tags, documents, cover art, strip) | `core/metadata/*` (`fields`, `codec` + `codecs`, `jpeg`/`png`/`webp`/`tiff` over `exif` + `exif_fields`, `xmp`, `iptc`, `audio` + `audio_payload` + in-house `asf`, `zip_document` (`openxml`, `epub`), `planner`, `executor`), `core/fs/staged_replace.rs`, `core/undo/metadata_edit_record.rs`, `app/side_pane/metadata_pane.rs`, ui: `metadata_pane.cpp`, `bb-test-support` (`metadata_fixtures*`), `tools/ui/metadata.bbs` |
| Metadata for types with no codec (Windows' property handlers) | `core/metadata/property_fallback.rs`, ui: `property_host.cpp` (the COM side), `tools/ui/metadata.bbs` ("written by Windows") |
| Side pane (none / preview / metadata) | `side_pane` in `app/browse/tabs.rs` / `commands.rs`, ui: `main_view.cpp` (three-button selector), `preview_pane.cpp`, `metadata_pane.cpp` |
| File dates (modified/created) | `core/timestamps/*`, `core/undo/timestamp_record.rs`, the two date rows of `app/side_pane/metadata_pane.rs` |
| Rename from tags (`{artist}`, `{taken:...}`) | `core/rename/{template,pattern,metadata_fields}.rs` |
| Search query language | `core/search/*`, `docs/search-indexing.md` |
| Content search (`content:`) | `core/search/{node (the `content:` term),content_reader,content_scanner,content_text,snippet}.rs` |
| Saved workspaces | `core/data/saved_workspace.rs`, `SavedWorkspaceRules` in `core/rules.rs`, `app/services/saved.rs`, ui: `sidebar.cpp`, Workspaces page of `settings_view.cpp` |
| Saved searches | `core/data/saved_search.rs`, `SavedSearchRules` in `core/rules.rs`, `app/services/saved.rs`, ui: `dialogs.cpp`, `sidebar.cpp`, Saved searches page |
| Elevated MFT indexer | `src/crates/bb-indexer`, `core/mft/*` (`reader`, `volume_indexer`, `host`, `client`, `service`, `presence`), `core/ipc/{index_endpoint,index_protocol,indexer_presence_lock}.rs` |
| Indexer banner / sign-in task | `core/mft/{banner_rules,autostart}.rs`, `app/services/index.rs`, `shell/indexing.rs` |
| Change timeline ("What changed") | `core/changes/*`, `core/data/change_log.rs`, `app/tools/changes.rs`, History page of `settings_view.cpp` |
| Elevated file-op retry | `src/crates/bb-elevator`, `core/elevation/*`, `core/ipc/elevation_protocol.rs`, `shell/elevation.rs` |
| Launching other programs | `shell/launcher.rs`, `app/ops/launching.rs`, `core/launch/*`, `shell/run_as.rs` |
| Shell context menu (7-Zip, Git, TortoiseSVN...) | `core/shellmenu/*` (`keys`, `rules`, `built_in`, `layout`, `separators`, `static_verb`), ui: `shell_menu_real.cpp` (hosts the handlers), `ops_menus.cpp` (composer), Context menu page `settings_ctxmenu.cpp`, `harness/canned_shell_menu.h`, `tools/ui/shellmenu.bbs` |
| Startup / CLI / single instance | `core/cli/*`, `shell/single_instance.rs`, `core/ipc/instance_endpoint.rs`, `bb-ffi/src/startup.rs`, `app/requests.rs`, ui: `src/app/app/main.cpp` |
| Foreground raise | `shell/foreground.rs`, `ForegroundRaiseRules` in `core/rules.rs` |
| Default folder handler (shell) | `core/shell_integration/*`, `shell/folder_handler.rs` |
| Updates / legacy data | `shell/updates.rs`, `core/updates/mod.rs` (`ReleaseChannel`), `core/settings/legacy.rs` |
| Preview pane (incl. hex/raw) | `core/preview/*` (`classifier`, `text_reader`, `hex_reader`, `syntax`), `app/side_pane/preview.rs`, ui: `preview_pane.cpp`, `code_view.cpp`, `image_view.cpp`, `font_view.cpp` |
| Video playback in the preview | `core/preview/playlist.rs`, ui: `media_player.cpp`, `media_pane.cpp` (Media Foundation `IMFMediaEngine`) |
| Animated GIFs in the preview | `core/preview/animation/*`, `app/side_pane/gif_player.rs`, ui: `image_view.cpp`, `tools/ui/gif.bbs` |
| Archives (zip/7z/tar/rar) | `core/archives/*` (`archive_path`, `reader`, `index_builder`, `writer`, `edit_*`, `extract_*`) |
| Commands / keyboard shortcuts | `core/commands/*` (`catalog`, `chord`, `keymap`, `verbs`), `app/commands/mod.rs` (`why_not_now`), `app/commands/keys.rs`, ui: `window.cpp` (key -> chord), `command_ui.cpp`, `tools/ui/{keys,keyboard,commands}.bbs` |
| Menu bar / toolbars | `core/commandbars/*`, `app/commands/bars.rs` (`CommandBarService`), ui: `ui/popups/menu_bar.cpp`, `command_ui.cpp`, `tools/ui/command-bars.bbs` |
| Command palette | `core/commands/{palette,matcher,goto}.rs`, `app/commands/palette.rs`, ui: `palette_view.cpp`, `tools/ui/palette.bbs` |
| Settings page | `app/services/settings.rs`, `core/settings/*`, ui: `settings_view.cpp`, `tools/ui/settings.bbs` |
| Theming | `core/theming/*` (`catalog`, `resolver`, `system`), `app/services/themes.rs`, ui: `gfx/palette.cpp`, `gfx/tokens.h` |
| App icon | `tools/icons/build-app-icon.ps1` -> `src/app/app/app.ico` |
| Icons | `tools/icons/icons.txt` (the mapping) -> `src/app/gfx/icons.gen.h` (generated) via `build-icons.ps1` |
| Columns (file list) | `core/columns/*`, `app/browse/columns.rs`, ui: `shell_props.cpp` (shell property values), `ui/collections/list_view.cpp` |
| Tabs / panes / layout | `app/browse/tabs.rs`, `app/session.rs`, `app/api/tabs.rs`, `core/layout/*`, ui: `main_view.cpp` |
| Flat branch view (Ctrl+B) | `FlatViewRules` in `core/rules.rs`, `SearchService::list_subtree` in `core/fs/search_service.rs`, `app/browse/tabs.rs` |
| Thumbnail tiles / scrolling | `ui/collections/tile_view.cpp` (uniform tiles by arithmetic), `thumbs.cpp` (4-slot shell gate), `tools/ui/tiles.bbs` |
| UI test harness | `harness/*` (`runner*.cpp`, `session.cpp`, `sandbox.cpp`, `crash.cpp`), `tools/ui/*.bbs`, `.claude/skills/verify` |
| FFI | `include/bbcore.h` (generated), `bb-ffi/src/*`, `app/events.rs`, `src/app/views/bridge/model.h` (JSON models) |
| Benchmarks / performance evidence | `src/crates/bb-bench` (`run`, `ui`, `startup`, `compare`, `baseline`, `report`, `corpus`), `core/benchmarking/*`, `bench/baselines/<machine-key>.json`, `docs/performance.md` (generated), `tools/ui/bench-*.bbs`, `shell/startup_trace.rs`, `bench/README.md` |

## Native gotchas

- **Static CRT on both sides.** C++ is `/MT` (`CMAKE_MSVC_RUNTIME_LIBRARY`), Rust is
  `+crt-static` (`.cargo/config.toml`). They must match or the staticlib link yields duplicate CRT
  symbols and heap-crossing bugs. A side effect is no VC redistributable in `Setup.exe`.
- **The window starts on WARP and moves to the GPU later** (`main.cpp`, `gfx::HardwareDeviceLoader`,
  `Window::use_hardware_when_ready`). A hardware `D3D11CreateDevice` loads the GPU's driver and took ~180 ms
  here, 80% of the time to first listing; WARP takes ~10 ms, on a thread of its own beside `bb_app_create`. The
  hardware device is made on a thread of its own and **started only after the window is shown** (the two share a
  lock inside D3D, and started first the driver load held WARP up for its whole length). When it lands the window rebuilds exactly as after a device
  loss (`GfxDevice::adopt_hardware` bumps `generation()`; every surface rebuilds on the next `render()`
  because a `Surface` remembers its generation). **A window shows its swap chain through a DirectComposition
  visual** (`gfx::CompositionHost`), not as the HWND's own: a chain is put on screen by its first present,
  replacing the one before it in a single commit, and `Window::retired_` keeps the old surface alive until
  the new one has presented. An HWND chain has to be destroyed before the next can exist, which left the
  window blank for the whole first paint on the new device - the flash at ~240 ms. For the same reason the
  first frame is painted *before* `show()` (`paint_first_frame`), not by the first `WM_PAINT`, after
  `await_first_listing` has taken in the rows (they land ~30 ms before it, and their event would otherwise wait for
  the message loop and a second whole frame). A maximised window is cloaked and maximised first, so that one frame
  is at its real size. **The first frame's ~30 ms is WARP's own first rasterisation**, about 7,000 page faults
  against 1,500 for an identical second frame (~7 ms): warming it from a thread (any size of target, the same
  device context, no WARP threading) and full LTO were all measured and moved nothing, and delay-loading the
  drawing DLLs only moved time from before `main` to after it. Popups keep
  HWND chains; `~SwapChainSurface` flushes its device's context, without which DXGI still counts the old HWND
  chain and the next one is refused with `E_ACCESSDENIED`. The startup trace waits for `gfxSettled` before
  reading memory, since a snapshot taken mid-load is mostly the driver. `BERTBROWSER_DEVICE=hardware`
  restores the old order for comparison.
- **The FFI is UI-thread only; the Rust event sink may only enqueue.** Every `bb_*` call comes from the
  UI thread and returns without blocking; results arrive as events. The sink runs on Rust threads and
  does one thing: `PostMessage(hwnd, WM_BB_EVENT, ...)`. Rust never re-enters C++ any other way. Every
  export is `catch_unwind` -> `BB_PANIC` (message via `bb_last_error()`); strings are UTF-16 `BbStr`
  borrowed in, Rust-owned views or `bb_str_free` out; stale ids answer `BB_STALE`.
- **`include/bbcore.h` is generated by cbindgen during the build and committed.** Commit the
  regenerated header with the Rust change that caused it; never discard it, never hand-edit it.
- **Widgets hold theme token ids, never brushes.** `gfx::Palette` owns the brushes and rebuilds them on
  a theme change or device loss, so nothing that draws hears about either. No literal colors in C++
  either - always a token.
- **COM objects Rust creates live on STA threads Rust owns** (apartment-threaded, with a deadline).
  Shell-extension COM in C++ runs on the UI thread, for the same reason.
- **The harness is a separate exe, never a mode of `BertBrowser.exe`.** `bb-harness` links
  `bbcore_harness.lib` from `bb-ffi-harness`, which holds the test fixtures and the fakes; the
  product's `bbcore.lib` must never include them. Teardown must `UiRoot::clear_content()` before
  `bb_app_destroy` (the tree owns handles the core closes - it crashed otherwise). The harness device
  is created without `D3D11_CREATE_DEVICE_VIDEO_SUPPORT`: WARP refuses the flag.
- **cargo gets a native-spelled target dir** (CMake `file(TO_NATIVE_PATH)`): forward slashes broke
  `embed-manifest` with LNK1104.
- **mimalloc is the global allocator** of `bb-ffi` (measured twice as fast across Tier A). `bb-bench`
  swaps in a counting allocator, so Rust allocation counts are deterministic.
- **Single instance**: mutex `Local\BertBrowser.<sid>` decides who is first; the first copy listens on
  `instance.pipe` for the hand-off (`shell/single_instance.rs`, `core/ipc/instance_endpoint.rs`).
- **Old C# data is moved, never deleted.** `LegacyData::migrate` (`core/settings/legacy.rs`) runs before
  the DB opens and moves `bertbrowser.db*`, `settings.json`, `themes\`, `templates\` into
  `legacy-<yyyymmdd>\`. The data dir stays `~\.bertbrowser` (the installer deletes `%LOCALAPPDATA%\BertBrowser`,
  so nothing of the user's may live there).
- **The UI is checked offscreen through the harness**: `BitmapSurface` on WARP re-renders the tree;
  input is posted, never synthesised. Dialogs are shown modelessly and captured. A `shot` that is one
  flat colour fails. The harness asserts every hazard seam is faked after create (launcher, folder
  handler, elevation, prompts, index) - keep that when adding a seam (`app/seams.rs`).
- **Tool windows are windows, dialogs are cards.** Disk usage, duplicates, changes, checksums, file
  compare, history, transfer and the theme editor are modeless owned `platform::Window`s
  (`MainView::is_tool_window`, `views/tools/tool_window.cpp`) and never set "a dialog is up", so the
  folders stay live; everything else is a modal card in the `DialogLayer`. A run reports dialogs
  instead of opening them unless `live-dialogs on`.
- **The product's confirm and notice seams park a worker on the window** (`Questions`,
  `Model::Confirm`/`Notice`, the `confirm`/`notice` dialogs). Ask from a worker thread only: a
  question on the UI thread could never be answered, and `UiPrompts` refuses it. An undo walk can park
  on an elevation offer while holding the history, so what the UI thread reads of it is `try_lock`.
  An owned tool window always stands above the main window, so a card asked from one (Duplicates'
  delete confirmation) hides that window while it is up.
- **Rebuilding a widget tree from inside one of its own handlers frees the handler's widget.** Activating
  a tab rebuilds every pane (strip and all), so the tab strip activates on *release*, after a drag has
  had its chance; Settings pages rebuild through `main_.defer`, never from a button's `on_click`.
- **One whole-PC search box, in the title bar** (`MainView::title_search_`), and it is the *active*
  tab's query; each tab's own box is folder-local. A tab only touches the title box while it is active.
- **An indeterminate `ProgressBar` animates for as long as it is indeterminate, shown or not** (its
  timer starts on paint). Hide one by turning indeterminate off too, or a 33 ms timer runs for ever.

## Cross-cutting gotchas

- **Path keys**: every DB path column stores `PathKey::canonicalize()` output (uppercased, no trailing
  separator except drive roots); use `prefix_bounds(dir)` for subtree range scans instead of `LIKE`. A row
  that isn't canonicalized breaks every subtree query silently. `is_under` re-canonicalizes and allocates
  per call - one question only; a loop over keys uses `is_under_key` / `has_ancestor_key_in`.
- **Selecting many rows is one `bb_tab_select`** (selection lives in Rust; one call per gesture), never a
  loop of single selections: invert over 10,000 rows was 9.5 s that way, 0.2 s as one.
- **Directory sizes are never computed on demand.** All numbers come from `dir_size_cache`, filled by
  the MFT pass. A missing row means *unknown* and must render blank, **never zero**.
- **Virtual (in-archive) paths must never reach a `PathKey`-keyed table.** `C:\x\a.zip\src` is a real
  Windows path syntactically, so it *would* land inside `prefix_bounds(C:\x)` - bookmarks, search and
  disk-usage each explicitly refuse a virtual root.
- **The index helper outlives the app.** Closing a window ends a *session*, not the helper - which is
  why reopening costs no UAC prompt and no rebuild. Easy to undo by accident: `MftIndexClient`'s drop
  must **not** send `Shutdown` (only `stop` may); the pipe name is well-known per user (`IndexEndpoint`),
  because a helper started under a previous app has to find the next one; every session **replays**
  `Building`/`Complete` before `Ready`; and one helper per user is enforced by a mutex
  (`IndexerPresenceLock`) because two would tail one journal into one database. The app never launches
  one at startup - it attaches, and a banner offers the prompt (`IndexerBannerRules`). Nothing supervises
  the helper, so it stops itself when the schema, its own exe, or `BertBrowser.exe` beside it changes
  (`bb-indexer/src/self_checks.rs`).
- **The change log (`fs_change`) is off by default and written only by the index helper's USN tail**,
  under a `ChangeLogPolicy` the app pushes over the pipe (`IndexVerb::Record`, one integer, never a
  path). Nothing reads the journal on demand; recording starts once a volume's build completes; rows
  carry the *record's* timestamp. The recorder must exclude the data directory and an empty flush must
  never touch the DB, or every DB write logs itself for ever. Turning the setting off wipes the table on
  both sides. **Recording continues while the app is closed** (the helper keeps the last policy) -
  deliberate, and stated on the settings page. Any feature that keeps history follows the same rule: off
  until the user turns it on, on its own Settings page, with a way to clear it.
- **One program-launching door**, `bb-shell` `launcher.rs` (`ShellExecuteExW`). A second call site is a
  bug. The carve-outs: the Indexer registers its sign-in task through Task Scheduler's COM API (starts no
  process), and `IContextMenu::InvokeCommand` in `shell_menu_real.cpp` (what 7-Zip starts is 7-Zip's
  code). Static registry verbs - "Git Bash Here" - still go through the launcher.
- **The shell context menu is composed per extension, never asked of the shell whole.** The app
  enumerates `shellex\ContextMenuHandlers` and `shell\<verb>` under the families `ShellMenuKeys` picks,
  hosts each COM handler in its *own* private `HMENU` (`shell_menu_real.cpp`) and mirrors it into the
  app's own D2D menu items. `CDefFolderMenu` would hand back Cut/Copy/Delete/Rename/Properties alongside
  the extensions with no way to tell whose item is whose - and the per-extension checklist on the Context
  menu page needs exactly that attribution. Consequences: the menu is in-tree, so the harness photographs
  it detached and a native `TrackPopupMenuEx` is never involved; `WM_INITMENUPOPUP` is delivered by hand
  through `IContextMenu2/3::HandleMenuMsg`; a session owns the COM objects, PIDLs and `HMENU`s and is
  released **after** the menu closes, deferred, because the click that calls `InvokeCommand` has to run
  first - and only that session; all on the UI thread (apartment-threaded extensions); never offered
  inside an archive or over a search result's empty space. Everything is shown unless unticked, so a
  newly installed extension appears without a visit to Settings, and `hidden_after_save` keeps an id hidden
  while its extension is uninstalled. The harness swaps in `canned_shell_menu.h`, so a run loads no
  foreign code. The app's own entries are unticked the same way (`BuiltInMenuItems`; an unregistered id
  throws rather than quietly showing), `built_in::apply` runs *first* and `separators` tidying *last*.
  **Order is `AppSettings.context_menu_layout`, one token list for both menus** (`shellmenu/layout.rs`),
  laid out by the composer in `ops_menus.cpp` on every opening; layout is order only - shown/hidden stays
  in the hidden lists and `CustomCommandDefinition.show_in_menu` - and `@more` is where anything unplaced
  lands. On the Settings page the preview rows *are* the membership.
- **The single-instance hand-off defeats the foreground lock on purpose, so it must ask before using
  it.** `shell/foreground.rs` only takes the foreground when `ForegroundRaiseRules` says nothing is full
  screen; otherwise it flashes the taskbar button and does not even un-minimize. The hand-off is not a
  user gesture: the app is the registered handler for `Directory` and `Drive`, so *anything* that
  shell-opens a folder starts a second copy that grants its foreground rights over.
- **Every shortcut is a `CommandCatalog` row, and there is one key dispatcher.** No per-control key
  handlers: the window turns the key into a `BbKeyChord` (C++), `bb_key_dispatch` /
  `KeymapRules::dispatch` says whose it is given what has focus (Rust), and the command registry runs it.
  A catalogue id with no handler (or the reverse) is an error at startup. The dispatcher sees a key
  *before* the focused control, so protection of editable text is a rule about the *chord*
  (`FiresInEditableText`, pinned in `default_keymap_parity_tests`), not the command. A claimed chord is
  marked handled **even when its command is unavailable**, or it falls through to type-ahead; keys reach
  the window behind a modal (`IsContentBlocked`) and from inside a menu or popup, and both must stand the
  dispatcher down, as must the Keyboard page's recorder. A held key runs its command once unless
  `RepeatsWhenHeld` says it is a step. File verbs are gated in **one** place, `FileVerbRules` (the menu,
  a key and the palette all go through it). Text that names a key asks the keymap's gesture text
  (`keymap_gesture`), never a literal. `KeymapService` is the keymap's only writer and writes at once.
- **The menu bar and toolbars are data, and an item is only a command id.** The arrangement is a
  `CommandBarSet` in `AppSettings.command_bars`; **null means the default**, and one equal to the
  default is written back as null, so an uncustomised user gets a later build's additions - a customised
  one is never edited behind their back (the picker's "not placed anywhere" is where a new command turns
  up; the palette reaches everything). `CommandBarService` is its only writer and writes at once; saved
  arrangements live in the settings file, not the database, because a migration makes every running
  index helper exit. `commands.rs::why_not_now` is the **one gate** - the registry's answer plus "a dialog
  is up" and "Settings is open" - for a menu row, a toolbar button and a palette row, and running from a
  bar asks it again because a button is only as fresh as its last refresh. The title bar stays live under
  a modal. Default top-level access keys avoid Alt+D/P/M (pinned in `command_bar_rules_tests`); the
  harness cannot send Alt, `menu main <Menu>` renders a dropdown detached. A caption marks its key with
  `&` and anything from data is `AccessKeys::escape`d first.
- **The palette's empty state is never the catalogue.** Pinned, then this session's recents, then
  `PaletteSuggestions` for what is selected, then one row per category (`PaletteRules`, ordering in
  `palette_rules_tests`). Recents live in memory only: a list of what was run is a history, and a
  persisted one would owe the opt-in switch every history feature here has. Pins are saved. Unavailable
  commands stay listed with their reason. It is an in-tree overlay (`popup_layer`), so `shot` photographs it.
- **The app is `asInvoker`** (manifest in `src/app/app`). Only the two elevated helper exes touch an
  administrator token. Don't reintroduce `requireAdministrator` on the app to fix an access-denied error -
  a folder the app can't read, Explorer can't either.
- **Every executor (transfer/rename/delete/new-item/archive-edit) has the same shape**: a pure `*Planner`
  deciding through a probe trait (testable without disk), an `*Executor` that re-applies the plan's rules
  against live disk state before writing, and "one item's failure never affects the others." Nothing ever
  does `remove_dir_all` on user data - use `core/fs/directory_removal.rs` (handles junctions correctly).
- **The right-drag verb menu is for our own drags only.** A foreign drop has to report an effect back
  before `Drop` returns and cannot wait on a menu, so an external right-drag keeps `drop_in`'s answer. The
  right button is remembered from the *press* (by the drop it is generally already up), and the menu is
  **posted**, not opened inside `Drop` - `DoDragDrop`'s modal loop still owns the mouse there.
- **The queue holds `is_transferring` for its whole drain, and pause lives in the progress coalescer.**
  That flag means "this app is writing", not "a transfer is running": ten operations raise it, and the
  four long ones (drop/paste, extract, compress, archive-edit) queue behind it. Releasing it between
  jobs would let a rename slip into the gap and push onto the undo history out from under a queue the
  user is watching, and would make the harness call a six-job drain finished five times over. Pausing is
  a `PauseGate` waited on in the progress callbacks, not in `IFileCopier`: `CopyFileExW` invokes its
  progress routine synchronously, so a callback that blocks *is* a pause in the middle of one large file.
  `PauseGate::wait` returns false rather than panicking (a panic must not unwind across the FFI boundary
  out of a callback Windows is still inside); the callback body is `catch_unwind` -> `PROGRESS_CANCEL`;
  silent staging takes no gate because staging must finish once started; a paused run holds a
  half-written destination open so every other write stays blocked; the harness's settle treats a paused
  queue as quiescent (`bb_app_is_busy`); and the progress view stops its stopwatch, or `TransferRate`
  turns a five-minute pause into an invented stall.
- **Only the conflict *dialog* was ever single-answer.** `TransferExecutor`, the elevation IPC and
  `SyncPlanner` take a per-path map of `ConflictResolution`. Asking goes through the conflict-prompt seam
  inside the drop path, so a drop and a paste reach one dialog; Rust raises `NeedsAnswer{request_id, ...}`,
  blocks the op thread, and C++ answers with `bb_answer`. `None` means *ask*; an empty map means "nothing
  needs an answer". A clash is not always with disk: `TransferPlan::earlier_claimant_of` names the other
  incoming item, because probing disk for a name nothing has written yet answers "missing".
- **A folder landing on a folder of the same name is merged, and the folder itself is never asked
  about.** `TransferMergeExpander` replaces that one item with the descendants that really clash, each
  named by its path relative to the drop. It runs **once, at the drop, off the UI thread** - never inside
  `TransferPlanner::plan`, which the drag-over check calls constantly and which takes the narrower probe so
  it *cannot* enumerate. Consequences: the walk descends only where **both** sides have a real directory,
  so nothing invents a folder nobody selected; **a link on either side is always a leaf** (a junction at
  the destination pointing back into the source would merge a folder into itself by a path `revalidate`
  never inspects); a move prunes the source folders it emptied (non-recursive `remove_dir` only) and
  `undo` recreates exactly those, so "its original folder is gone" keeps meaning *the user* deleted it;
  the clash's two sides are carried on the plan so a dialog of five thousand rows does no disk IO;
  "identical" is `CompareEquality` at **`Strict`** tolerance, never `Loose`; and past
  `TransferMergeLimits` the folder keeps its wholesale question.
- **Undo is a session-only, LIFO history** (`UndoStack`) of moves, renames, deletes, merged pastes,
  archive edits, syncs, metadata edits and date changes, with redo and "undo/redo to here". Data is
  committed only when an entry's record is `retire`d: when the budget (steps + held bytes) releases it,
  when a new operation discards the redo branch, on Clear, and on window close. The budget never releases
  the next undo, however large. Each operation records itself after its elevated retry, before its
  refresh. **Records are immutable and every step returns a new one holding only what actually moved**,
  hence undo results carry `reverted` and a partial undo is never replayed. A walk stops at the first step
  that is not clean. Redo re-plans from the record (`transfer/redo`, `delete/redo`): same final names,
  displace only what was displaced before, Skip everything else - never number or replace a newcomer.
  **A copy's outcome reports `can_undo == false`** (it means "`undo()` will work", and `undo` refuses a
  copy); `ConflictResolution::Overwrite` exists because a copy that displaced something with no record
  kept would strand it in staging for ever - only a sync or a **merged** drop/paste may ask for it.
  `undo_copies` removes only what still matches the `Written` `EntryStamp`, which also makes a replayed
  record harmless. It must never be elevated: `ElevationHost::undo_transfer` hardcodes a move, so handed
  a copy it would move the pasted files back into their source - `IUndoHost` has no copy-undo member. An
  archive-edit undo **stages** the edited container instead of deleting it, and both directions refuse a
  container whose stamp changed. Held bytes are file stats plus `dir_size_cache`, falling back to a capped
  walk of our own staged copy - an estimate for the budget only, never shown as a folder size. On by
  default, unlike the change log, because nothing it holds outlives the session.
- **The preview and the metadata pane are one three-state choice (`SidePane`), not two switches.** They
  share a column, a splitter and a remembered width. The metadata pane follows the selection and reloads
  only when the selected paths *or their modified times* change - a refresh that changes neither must not
  wipe what is half typed. It never writes: Apply goes through the shell. After a write the list has
  reloaded with nothing selected, so the pane re-selects what it edited or it would answer its own edit by
  going blank. The file's modified/created dates are two more rows, applied through the timestamp executor
  **after** the metadata, since rewriting a file is what moves its modified date.
- **A metadata edit is only swapped in once its payload digest matches the original's.**
  `IMetadataCodec::payload_digest` hashes everything that is *not* metadata, and the executor writes a
  sibling, reads it back, compares that digest and the fields, and only then swaps - a codec bug costs an
  error message, not a photograph. Easy to undo: the audio digest (`audio_payload`) is **our own reading,
  never `lofty`'s** (a check that asks the suspect where to look is not one), and a container with no
  locator there cannot be edited however well `lofty` reads it; WMA/ASF is in-house (`metadata/asf`);
  for MPEG-4 hashing `mdat` is not enough, it also hashes what each `stco`/`co64` entry points at;
  `ExifBlock` **never relocates anything** (a MakerNote's offsets are absolute), it overwrites in place or
  appends, and zeroes what it abandons; a removal is verified by reading `has_location` back, and XMP that
  mentions GPS but will not parse counts as *having* one and is refused; an existing XMP packet is kept in
  step on every edit and never created; the swap is `StagedReplace` (`ReplaceFile`), not two renames,
  which keeps creation time, ACL, attributes and `Zone.Identifier`; held versions are hidden siblings,
  left in place when a step fails; an edit that produces identical bytes is `Unchanged`, not a history
  entry; no elevated retry (archives have none either). **Which fields a file takes is its codec's answer
  (`IMetadataCodec::fields`)** and `every_field_a_container_offers_is_one_it_keeps` pins each set. An Ogg
  stream is hashed by **packet, not page**; a WebP given its first EXIF gains a `VP8X` header and loses it
  when stripped; a TIFF *is* an `ExifBlock` end to end; a zip document is hashed by entry name and
  *uncompressed* bytes, an EPUB's package part by everything but `<metadata>`. `is_inside_archive` asks a
  path's **folder**, never its own name, because an EPUB is an archive and standing on one is not being in
  it. IPTC is kept in step like XMP, and a value its charset or length limit cannot hold is taken out of
  the record rather than written wrong. No codec: PDF (an incremental update cannot honour a removal), MOV,
  HEIC, camera RAW - those get Windows' own handler, unchecked (next). `lofty` is reached from `audio.rs`
  alone. Fixtures (`metadata_fixtures*`) are never written by the code under test.
- **A file no codec writes goes through Windows' property handler, and that is the one metadata write with
  no payload check.** The handler is only ever pointed at a **copy** beside the file (it writes in place),
  the fields are read back from the copy, and it is swapped in and held for undo like any rewrite. The pane
  says "Written by Windows" on every such file. A codec always wins (`MetadataCodecs::handles` is asked
  first); fields only - never a removal or a picture; **`IsPropertyWritable` lies on a read-only store**
  (a `.txt` answers yes to everything), so the question is asked of a store opened `GPS_READWRITE`, and
  because that open is exclusive the answer is remembered per extension and values are read through an
  ordinary shared store; a store that would not open is not remembered. Offering a field is the handler's
  promise, not ours: `Commit` can still refuse (it does for a tiny `.mov`), costing the copy and an error.
  Which property a field is, the star bands, and dates crossing as UTC are `WindowsPropertyFields`, in
  Core; the COM calls are `property_host.cpp`. The planner takes the offered fields as an argument.
- **A comparison's "same" is what authorises a delete**, so every doubt resolves away from it: a missing
  timestamp is `Unknown` and one `Unknown` descendant carries a whole subtree to `Unknown`.
  `dir_size_cache` deliberately never classifies a folder - equal totals do not mean equal trees.
- **One opener, `core/fs/read_only_file.rs`.** The share flags, the cloud-placeholder refusal and the
  reparse-point rule have three callers (hasher, digester, content comparison); a fourth copy is another
  chance for one to drift, silently both ways (too narrow and the app fights its own rename; too wide and
  a preview starts a multi-gigabyte download).
- **Checksums: two seams over one read loop, and the CRC-32 byte order.** `IFileHasher` stays SHA-256-only
  because the duplicate finder's safety argument is that answer authorises a delete; `IFileDigester` is the
  user-chosen one. `crc32fast` returns the checksum as a number and every `.sfv` states it big-endian -
  the rendering is guarded by a known-vector test because getting it wrong yields a plausible digest
  nothing else would catch. Digests are uppercase everywhere inside Core; casing is decided in one place
  (`ChecksumAlgorithms::is_written_lowercase`) and applied only when rendering.
- **A checksum file is untrusted input.** Every name in one is about to become a path this app opens, so
  `ChecksumPath::resolve` refuses anything rooted, device-prefixed or escaping the folder, and returns
  `None` rather than failing - a hostile line becomes a visible row in the report. Line order comes from
  the *extension*, never a sniff: `.sfv` is `name CRC`, the `*sum` family is `digest  name`.
- **Content settlement is one-directional, earned, and rewrites the `CompareResult`.** Identical bytes may
  raise `LeftNewer`/`RightNewer`/`Unknown` to `Same`; nothing else moves, and `Unreadable` is never
  evidence. It produces a **new** `CompareResult` rather than an overlay, because `SyncPlanner` reads
  verdicts straight off the result. Roll-ups are re-folded from scratch (`RollUp` only ever raises rank).
  A leaf `Differs` is unsettleable in practice: it means equal timestamps and *unequal sizes*.
- **`SearchNode::matches` (definition) and `write_sql` (optimization) must never disagree** - SQL may be a
  superset (re-checked per row) but never a subset. `ContentTerm` extends this to three-valued matching
  (`Yes`/`No`/`NeedsContent`) since content can't be answered from a column. `search_agreement_tests`
  holds it down.
- **`is_flattened` says the rows come from many folders; `is_flat_browse` says they still have one
  home.** A search sets the first alone, Ctrl+B sets both, and which one a guard reads is the whole point.
  The Folder column, the folders-first sort band, the watcher merge and a folder comparison follow
  `is_flattened` - a one-level watcher listing cannot be merged into a recursive one, so a flat view has
  no live refresh and F5 re-runs it. New, Compress and a drop on empty space follow the narrower
  `is_search_result`, because a flat browse has a "here" and a whole-PC search does not. Set the flag
  *before* beginning the flattened listing, since a comparison ends on it changing. Flat is deliberately
  **not** cleared by navigation, unlike search. It reads the disk and never the index - a browse surface
  must be true rather than instant - while the estimate behind the are-you-sure prompt does come from
  `dir_size_cache` (one primary-key lookup), because an estimate only decides whether to ask.
- **The thumbnail view lays tiles out by arithmetic, so tiles have to be uniform** (`ui/tile_view`: the
  fixed two-line caption height). Measuring every tile was 6 s a thousand rows and worse than linear. A
  thumbnail is a megabyte: shell calls go through a four-slot gate (`ThumbnailService::kSlots`), each
  result is posted to the UI thread and handed to the tile that asked, and the list asks only for the
  rows it paints (`bb_listing_row` fills views for visible rows at paint time only). `tools/ui/tiles.bbs`
  and `bench-tiles-5k.bbs` hold this down.
- **The UI thread never asks the shell about a file type** (`views/shell_icons`). A type's lookup (~2 ms) and
  picture (~1 ms) are done by a worker the first time it is seen; `file()` answers null meanwhile and the row
  draws its glyph. A drain repaints the whole window, so the worker only posts when its queues are idle or
  every 50 ms. The harness's `settle` waits for the icons (so a `shot` has them); `time` and the startup
  timer turn that off, since the icons are not how long the window took to answer. Doing the lookups inline
  was +25 ms on a 10k listing and +65 ms to first listing.
- **Nothing holds a file open** across previews, content search, or duplicate hashing - share
  read|write|delete everywhere, or this app's own rename/move/delete blocks itself. Cloud placeholders
  (`NotDownloaded`/`Offline`) are refused rather than silently hydrated (also true for shell-metadata
  columns - see `MetadataReadRules`).
- **Icons are named, never numbered.** A wrong picture is the one UI mistake nothing catches. Call sites
  say `icons::Back`, so a bad name fails at compile time. Add one by editing `tools/icons/icons.txt`
  and re-running `pwsh tools/icons/build-icons.ps1` - never hand-edit `icons.gen.h` - then **look at
  it** in a harness capture. Drawn outlines from Fluent UI System Icons (MIT), not the Segoe font.
- **Settings is a page of the main window, not a dialog, and it applies as you go.** It lies over
  everything under the title bar, at most 1280 wide and centred over a scrim that dims the folders and
  takes their clicks (a click on it closes the page); Tab cycles inside the layer. Only app-level
  shortcuts fire meanwhile. There is no Save: `bb_settings_set` writes on a 400 ms debounce, driven by an
  *allowlist* of persisted properties so selecting a row writes nothing, flushed on Back and on window
  close. A command or new-file type with a missing field keeps its whole list unwritten and says so in a
  strip rather than jumping pages mid-typing; only leaving takes the user to it. Each value is re-pushed
  only when it moved - the change-log policy push wipes the log whenever recording is off, so calling it
  on every keystroke elsewhere on the page is not harmless.
- **Matching Windows is the default only because there is no settings file.** `follow_system_theme ??
  !loaded_from_disk` - not `theme_id.is_none()`, which every existing user who never opened Settings also
  has. Initialize pins the answer on first run, or launch two finds a file and reads the null as "no". A
  system-driven switch goes through `apply_for_current_state`, never `select_theme`, because
  `select_theme` writes `theme_id` and the OS writing it would convert a following user into a pinned one
  at sunset. High contrast wins over both slots and never consults them; the light/dark bit is kept
  *underneath* it. A run poses all of this through the fake system appearance and starts pinned, so
  screenshots do not depend on the developer's own desktop.
- **Three benchmark tiers write one schema, and only `bb-bench` writes it** (`core/benchmarking`,
  schema v2). Tier C's headline is `listingPainted`, not `firstListing` (which is only "nothing is loading"). Tier A is a hand-rolled runner over `bb-core` with a counting allocator, Tier B times whole
  scenarios through the harness (`tools/ui/bench-*.bbs`; the `time` verb wraps any verb), Tier C launches
  the real exe. **CI gates allocations only** (deterministic); time gates run locally at 15% over a noise
  floor, and so does process memory (managed heap tight, private bytes loose). A deliberate slowdown
  re-records the baseline and regenerates `docs/performance.md` (`report`) in the same commit, with
  the reason in the message. Easy to undo: an allocation tolerance is for benchmarks over SQLite, threads
  or disk only - loosening a pure one defeats the gate; `settle-thumbnails` is a verb rather than part of
  `settle`, or every script would wait on shell COM calls; the synthetic corpus lives on drive `Q:` so it
  can never be mistaken for a real folder, and `--real` reads a daily SQLite backup, never the live DB.
  **Tier C is the one sanctioned launch of `BertBrowser.exe`**, only via `bb-bench startup`: under
  `BERTBROWSER_STARTUP_TRACE` the app parks its window offscreen and skips the index attach and the
  change-log policy push - the single-instance mutex and the helper's pipe are per user, not per data
  directory, so a scratch-dir launch would otherwise push "recording off" at the user's real helper and
  wipe their change log. Nothing a benchmark motivates may change the `matches`/`write_sql` agreement.
- **When editing a doc file with load-bearing prose** (this file, `docs/*.md`), keep new content as terse
  pointers/gotchas, not a re-narration of the code - the code and its tests are the source of truth; this
  file is a map, not a manual.
