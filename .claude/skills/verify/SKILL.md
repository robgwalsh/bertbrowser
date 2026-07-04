---
name: verify
description: Build, launch, and drive BertBrowser (WPF) for end-to-end verification via UI Automation.
---

# Verifying BertBrowser

## Build & launch

```powershell
dotnet build bertbrowser.sln            # TreatWarningsAsErrors: any warning fails
# exe name is BertBrowser.exe (not BertBrowser.App.exe):
src\BertBrowser.App\bin\Debug\net8.0-windows\BertBrowser.exe "<start-directory>"
```

Point the start-directory argument at a throwaway tree under `%TEMP%` — the app
writes to the real DB at `%LOCALAPPDATA%\BertBrowser\bertbrowser.db` (search index
rows for temp trees are harmless but persistent).

## Driving the UI (System.Windows.Automation from PowerShell)

- Find the window by Name `BertBrowser` (RootElement children).
- Find controls by **AutomationId** (`SearchBox`, `FileListView`, `PathBox`) — most
  reliable. The search box also has AutomationProperties.Name "Search this folder".
- Status bar: ControlType.StatusBar → descendant Text elements; the status string
  (e.g. `2 result(s) for 'report' under … — indexed`) is the best programmatic
  assertion surface.
- File-list rows are ControlType.**DataItem** (GridView mode), not ListItem; scope
  the FindAll to the FileListView element or you'll count breadcrumb items.
- Setting text: `ValuePattern.SetValue` occasionally garbles WPF TextBox content —
  always read the value back and retry until it matches. Real keystrokes
  (`SendKeys` after `SetFocus`) also work but require foreground.

## Gotchas on this machine

- A fullscreen game is often running: `CopyFromScreen` screenshots capture the game,
  and `SetForegroundWindow`/`SendKeys` steal the user's focus. Prefer the quiet
  path: UIA `ValuePattern.SetValue` (no focus needed) + `PrintWindow(hwnd, hdc, 2)`
  (PW_RENDERFULLCONTENT — captures occluded WPF windows correctly).
- Kill the process instead of closing the window if you don't want the app to save
  `LastPath`/window bounds into `%LOCALAPPDATA%\BertBrowser\settings.json`.

## Flows worth driving

- Type in the search box (≥2 chars) → status shows `N result(s) … — indexing…/indexed`.
- Wildcards `*.xlsx`, literal `[` chars, 1-char query (stays in browse mode), Esc clears.
- Rename a file on disk under an indexed root → re-search finds the new name within
  ~1s (FileSystemWatcher patches the SQLite index).
