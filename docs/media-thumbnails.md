# Media thumbnails

How the tile view puts previews of photos and videos on screen without stalling. The short version:
**BertBrowser never decodes an image.** It asks Windows for a thumbnail that has almost always
already been generated, does it off the UI thread, and shows a file-type icon in the meantime.

## The four ideas

| Idea | Why it matters |
|---|---|
| Ask the shell, don't decode | `IShellItemImageFactory` reads Windows' own thumbnail cache; a folder Explorer has already seen is nearly free |
| Freeze every bitmap | A frozen `ImageSource` is legal to create on a worker thread and hand to the UI, with no copy |
| Show something instantly | A cached extension icon fills the tile on the first frame; the real preview swaps in when it lands |
| Fetch once, at one size | Every zoom level renders from the same 512 px bitmap, so dragging the zoom slider refetches nothing |

## Not decoding images ourselves

`Interop/ShellThumbnails.GetThumbnail` is the whole extraction path:

```
SHCreateItemFromParsingName(path) → IShellItemImageFactory
    → GetImage({512, 512}, SIIGBF_RESIZETOFIT) → HBITMAP
    → Imaging.CreateBitmapSourceFromHBitmap → Freeze()
```

Three details in there carry weight:

- **`SIIGBF_RESIZETOFIT` (flag 0) means "a preview if one exists, otherwise the file-type icon."**
  One call therefore covers JPEG, HEIC, AVIF, PSD, PDF, video first-frames, and anything else with a
  registered thumbnail handler — including formats WPF's own `BitmapDecoder` cannot open. Codec
  support is whatever the machine has installed, not something the app has to ship or track.
- **Windows caches the result.** `GetImage` consults the per-user thumbnail cache
  (`%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db`) and only extracts on a miss. This is
  the single biggest reason tile view feels quick: the expensive work — decoding a HEIC, seeking a
  video to its first keyframe — happens once per file per size class, ever, and is shared with
  Explorer. A folder you have already browsed in Explorer is warm before BertBrowser opens it, and
  vice versa.
- **The unmanaged handles are cleaned up in `finally`** — `DeleteObject` on the HBITMAP,
  `ReleaseComObject` on the factory. A thumbnail per tile per folder leaks GDI objects fast
  otherwise, and GDI handle exhaustion shows up as the whole window failing to render, not as a
  missing picture.

Failure is expected and cheap: `COMException`, `FileNotFoundException` and `ArgumentException` are
caught and turned into `null` — no thumbnail handler, file deleted mid-scroll, unreadable media —
and the caller simply keeps the icon it was already showing.

### Freezing is what makes the threading legal

WPF `Freezable`s belong to the thread that created them unless frozen. `source.Freeze()` before
returning is what allows `GetThumbnail` to be called from a thread-pool thread at all, and it means
the bitmap is handed to the UI by reference rather than copied or re-created on the dispatcher. The
same trick is what lets `ShellIcons` share one icon instance across every row with that extension.

## The placeholder chain

`FileItemViewModel` exposes two image properties, and a tile binds the second:

**`Icon`** — the small shell icon, in three tiers by cost:

| Kind | How it resolves | Where it's cached |
|---|---|---|
| Folders and ordinary files | `SHGetFileInfo` with `SHGFI_USEFILEATTRIBUTES` — registry only, **no disk access** | `ConcurrentDictionary` keyed `<dir>` / `<none>` / `.jpg` — bounded by the number of distinct extensions on the machine |
| `.exe`, `.ico`, `.lnk` | Extracted from the file itself, which touches disk and can stall for *seconds* (a shortcut pointing at a dead network share) | Bounded LRU of 512, keyed by full path |

The first tier resolves inline on the UI thread, which is safe precisely because it never touches
disk — and doing it inline avoids a frame of flicker. The second tier is pushed to a worker
(`LoadIconAsync`) and raises `PropertyChanged` when it arrives, because a single dead `.lnk` bound
synchronously would freeze the window mid-scroll. The LRU is capped because an unbounded per-path
cache leaks steadily over a long session, and its factory deliberately runs *outside* the lock so
one stalled shell call cannot block every other icon thread.

**`Thumbnail`** — the large preview, always asynchronous:

```csharp
public ImageSource? Thumbnail
{
    get
    {
        if (!_thumbnailRequested)
        {
            _thumbnailRequested = true;
            _thumbnail = Icon;            // instant placeholder
            _ = LoadThumbnailAsync();
        }
        return _thumbnail;
    }
}
```

The `_thumbnailRequested` latch matters more than it looks: a property getter is called on every
binding refresh, and without it a re-render would queue a duplicate shell call each time. With it,
each view model fetches exactly once in its lifetime. And when the fetch fails, `LoadThumbnailAsync`
returns *without* notifying — so the icon placeholder simply stays, and a file with no preview never
shows an empty tile.

## One fetch serves every zoom level

The zoom slider drives `FileListViewModel.ThumbnailScale` (0–1), mapped to a tile width of 64–256 px
with a 5% dead zone so the smallest size is easy to hit rather than requiring an exact pixel.
`ThumbnailTileHeight` is that width through the configured aspect ratio, so the slider means the same
thing at every shape.

But the shell fetch is a **constant 512 px**, twice the largest tile. That is deliberate on two
counts:

- Downscaling stays sharp (`RenderOptions.BitmapScalingMode="HighQuality"` on the `Image`), and the
  scaling itself is a render-time operation, not a re-decode.
- **High-DPI displays would otherwise upscale.** A 256-DIP tile on a 150–200% display is 384–512
  *physical* pixels; fetching at the DIP size would hand WPF a bitmap smaller than the area it has
  to fill, and the result looks soft in exactly the situation where the user asked for the biggest
  thumbnails.

The consequence is the important part: dragging the zoom slider only changes two `Width`/`Height`
bindings. No fetch, no reload, no re-layout of the underlying data — which is why zooming is smooth
rather than a stutter of reappearing tiles.

## Only media files pay for any of this

`FileItemViewModel.IsMedia` tests the extension against a fixed set — the raster and vector image
formats (`.jpg`, `.png`, `.webp`, `.heic`, `.avif`, `.svg`, …) and the common video containers
(`.mp4`, `.mkv`, `.mov`, `.webm`, …). `ThumbnailTemplateSelector` renders those as tiles and
everything else — folders, documents, source files — as full-width rows that use the cheap `Icon`
path.

`FileListViewModel.LayoutBand` then sorts in bands ahead of the user's chosen column: directories
(0), non-media files (1), media tiles (2). Visually this reads as a list of rows followed by a
thumbnail grid. Practically it means a folder of 5,000 source files costs *zero* thumbnail work in
tile mode, and a folder mixing documents with photos only pays for the photos.

## Switching modes without rebuilding the list

`DirectoryTabView.ApplyViewMode` swaps four properties on the **same** `ListView` rather than
building a second control:

| Property | Details mode | Tile mode |
|---|---|---|
| `View` | the `GridView` | `null` — lets the item template take over |
| `ItemsPanel` | default virtualizing stack | `ThumbPanel` (a `WrapPanel`) |
| `ItemTemplateSelector` | unset — `GridView` supplies cells | `ThumbOrRowSelector` |
| `ItemContainerStyle` | `FileRowStyle` | `ThumbItemStyle` |

Reusing one `ListView` keeps selection, the context menu, double-click, drag-and-drop and type-ahead
working identically in both modes, with no duplicated wiring. A `_thumbnailViewApplied` guard means
the swap only happens when the boolean actually flips — dragging the slider *within* tile mode
churns nothing. Horizontal scrolling is disabled in tile mode so the `WrapPanel` is bounded to the
viewport width and tiles roll onto the next line.

One rendering note that is not about speed: the tile's `Border` sits on
`Theme.Thumbnail.TileBackground`, a pale colour, because shell thumbnails are authored assuming a
light background. Without it a transparent PNG or a high-key photo dissolves into a dark window.

## What is *not* optimized

Worth being straight about, since the rest of this document is a list of things that are:

- **Tile mode does not virtualize.** The `ListView` sets
  `VirtualizingPanel.IsVirtualizing="True"` and `VirtualizationMode="Recycling"`, and those apply in
  details mode — but tile mode's `ItemsPanel` is a plain `WrapPanel`, which is not a
  `VirtualizingPanel`, so the attached properties have no effect there. Every tile in the folder is
  realized on load, which means every media file's `Thumbnail` getter fires at once and queues a
  thread-pool work item. With a warm shell cache that burst resolves quickly; on a cold folder of
  several thousand videos it will not feel instant. WPF ships no virtualizing wrap panel, so fixing
  this properly means writing one.
- **No priority or cancellation.** Fetches are not ordered by what is actually on screen, and
  nothing cancels them when you scroll past or navigate away — the results are simply discarded with
  their view models.
- **No in-process thumbnail cache.** Leaving a folder and coming back builds fresh view models and
  refetches. The shell cache absorbs most of that cost, but it is a round trip per file, not free.
- **Memory.** A 512×512 bitmap per media file stays alive for as long as the list's view models do.

## Where the code is

| File | Role |
|---|---|
| `App/Interop/ShellThumbnails.cs` | The `IShellItemImageFactory` call, HBITMAP → frozen `ImageSource`, handle cleanup |
| `App/Interop/ShellIcons.cs` | Extension-keyed and per-file icon caches, the bounded LRU |
| `App/ViewModels/FileItemViewModel.cs` | `IsMedia`, the `Icon`/`Thumbnail` placeholder chain, the 512 px constant |
| `App/ViewModels/FileListViewModel.cs` | Zoom scale → tile size, aspect ratio, the sort bands |
| `App/Views/ThumbnailTemplateSelector.cs` | Tile vs. row per item |
| `App/Resources/Styles.xaml` | `ThumbPanel`, `ThumbTileTemplate`, `ThumbRowTemplate`, `ThumbItemStyle` |
| `App/Views/DirectoryTabView.xaml.cs` | `ApplyViewMode` — the details/tile swap |

None of this is in `BertBrowser.Core` and none of it is unit-tested: it is all shell interop and WPF
templating, which needs a real desktop session rather than xUnit. The one piece that *is* testable
lives in Core — `AspectRatio` (`AspectRatioTests`), since tile shape is a hand-editable line of
`settings.json` and a bad parse would hand WPF a `NaN` height.
