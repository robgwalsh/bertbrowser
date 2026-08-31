using System.Windows.Media;
using BertBrowser.App.Interop;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed partial class FileItemViewModel : ObservableObject
{
    public string Name { get; }
    public string FullPath { get; }
    public bool IsDirectory { get; }
    public DateTime ModifiedUtc { get; private set; }
    public string TypeName { get; }

    /// <summary>Hidden (own or inherited) — drives the dimmed-icon treatment.</summary>
    public bool IsHidden { get; private set; }

    /// <summary>Ghosted like Explorer when hidden.</summary>
    // 0.55, not the 0.45 this used to be: against a dark window the lower value reads as mud
    // rather than as a dimmed icon, and it still looks clearly dimmed on a light theme.
    public double IconOpacity => IsHidden ? 0.55 : 1.0;

    /// <summary>Files that get a real visual preview (images/videos). Only these render as
    /// thumbnail tiles; folders and other files always stay as rows.</summary>
    private static readonly HashSet<string> MediaExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".jpg", ".jpeg", ".jfif", ".png", ".gif", ".bmp", ".webp", ".tif", ".tiff", ".ico",
        ".heic", ".heif", ".avif", ".svg",
        ".mp4", ".mov", ".avi", ".mkv", ".wmv", ".webm", ".m4v", ".flv", ".mpg", ".mpeg",
        ".3gp", ".m2ts", ".mts",
    };

    public bool IsMedia => !IsDirectory && MediaExtensions.Contains(Path.GetExtension(FullPath));

    /// <summary>Path relative to the search root; only set in flattened search-results mode.</summary>
    public string RelativePath { get; }

    /// <summary>
    /// The listing entry this row was built from, for comparing against a fresh listing.
    /// </summary>
    /// <remarks>
    /// <para>
    /// A directory's <see cref="SizeBytes"/> is deliberately left out — it is hydrated from the
    /// size cache rather than read from disk, so including it would make every folder look changed
    /// the moment its cached total arrived, and the live refresh would rebuild rows for nothing.
    /// </para>
    /// <para>
    /// "Left out" is spelled <c>-1</c>, not <c>0</c>, because that is what a lister writes for a
    /// directory whose size it does not know. Writing <c>0</c> here meant every folder differed
    /// from its own fresh listing on every merge pass — the exact rebuild the paragraph above says
    /// this exists to avoid.
    /// </para>
    /// </remarks>
    public FileEntry ToEntry() => new(
        Name, FullPath, IsDirectory, IsDirectory ? -1 : SizeBytes ?? 0, ModifiedUtc,
        IsHidden ? FileAttributes.Hidden : FileAttributes.Normal);

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay), nameof(SizeSortKey))]
    private long? _sizeBytes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay))]
    private bool _sizeIncomplete;

    [ObservableProperty]
    private DateTime? _sizeComputedUtc;

    private ImageSource? _icon;
    private bool _iconLoaded;
    private bool _iconLoading;

    public FileItemViewModel(FileEntry entry)
    {
        Name = entry.Name;
        FullPath = entry.FullPath;
        IsDirectory = entry.IsDirectory;
        ModifiedUtc = entry.ModifiedUtc;
        IsHidden = entry.Attributes.HasFlag(FileAttributes.Hidden);
        RelativePath = string.Empty;
        // A directory's size is normally unknown at listing time (FileSystemService writes -1) and
        // arrives later from dir_size_cache. A lister that already knows it exactly — the archive
        // one, where the number was in the container's own directory — writes a real value, and
        // that is the difference the >= 0 test reads. Zero is a fact here, not a missing row.
        SizeBytes = entry.IsDirectory ? (entry.SizeBytes >= 0 ? entry.SizeBytes : null) : entry.SizeBytes;
        TypeName = entry.IsDirectory
            ? "Folder"
            : Path.GetExtension(entry.Name) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    /// <summary>Search-result mode: a real filesystem entry (file or directory) plus its
    /// parent path relative to the search root.</summary>
    public FileItemViewModel(FileEntry entry, string relativePath, ContentMatch? match = null)
        : this(entry)
    {
        RelativePath = relativePath;
        Match = match;
    }

    /// <summary>
    /// Where a <c>content:</c> search found its needle, or null. Search-results mode only, exactly
    /// as <see cref="RelativePath"/> is.
    /// </summary>
    public ContentMatch? Match { get; }

    // The matching line, split into the three runs the cell renders so the needle can be
    // highlighted. Doing the split here rather than in a converter keeps the view free of logic and
    // means the offsets are computed once per row instead of on every re-render.
    //
    // Clamped rather than trusted: these are offsets into a line the snippet already clipped, and a
    // cell that threw would take the whole list with it.
    public string MatchPrefix => Match is null ? "" : Match.Line[..Clamp(Match.MatchStart)];

    public string MatchText => Match is null
        ? ""
        : Match.Line[Clamp(Match.MatchStart)..Clamp(Match.MatchStart + Match.MatchLength)];

    public string MatchSuffix => Match is null
        ? ""
        : Match.Line[Clamp(Match.MatchStart + Match.MatchLength)..];

    /// <summary>The line number, shown dimmed beside the text. Empty when there is no match.</summary>
    public string MatchLineNumber => Match is null ? "" : Match.LineNumber.ToString();

    private int Clamp(int index) =>
        Match is null ? 0 : Math.Clamp(index, 0, Match.Line.Length);

    /// <summary>Fills size/modified/hidden from disk for a search result whose index row
    /// lacked them — MFT-built rows carry no size or timestamp. Intended to run off the UI
    /// thread before the item is bound.</summary>
    public void HydrateSearchMetadata()
    {
        // Raw-$MFT index rows already carry a real timestamp (and size); only the names-only
        // USN-enum fallback leaves them unset, so stat just those.
        if (ModifiedUtc != default)
            return;
        try
        {
            if (IsDirectory)
            {
                var info = new DirectoryInfo(FullPath);
                if (!info.Exists) return;
                ModifiedUtc = info.LastWriteTimeUtc;
                IsHidden = info.Attributes.HasFlag(FileAttributes.Hidden);
            }
            else
            {
                var info = new FileInfo(FullPath);
                if (!info.Exists) return;
                SizeBytes = info.Length;
                ModifiedUtc = info.LastWriteTimeUtc;
                IsHidden = info.Attributes.HasFlag(FileAttributes.Hidden);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            // Best-effort: leave the index's (empty) values in place.
        }
    }

    public ImageSource? Icon
    {
        get
        {
            if (_iconLoaded) return _icon;

            // Executables, shortcuts and icon files have their icon extracted from disk — a shell
            // call that can stall for seconds (e.g. a .lnk targeting a dead network share). Bound
            // through the UI thread it would freeze the whole window during scroll, so load it
            // off-thread and raise a change when it arrives. Directory/by-extension icons resolve
            // from the registry without disk access, so they stay inline (no flicker).
            if (ShellIcons.IsPerFileIcon(FullPath, IsDirectory))
            {
                if (!_iconLoading)
                {
                    _iconLoading = true;
                    _ = LoadIconAsync();
                }
                return _icon; // null placeholder until the real icon loads
            }

            _iconLoaded = true;
            return _icon = ShellIcons.GetIcon(FullPath, IsDirectory);
        }
    }

    private async Task LoadIconAsync()
    {
        var image = await Task.Run(() => ShellIcons.GetIcon(FullPath, IsDirectory));
        _icon = image;
        _iconLoaded = true;
        OnPropertyChanged(nameof(Icon));
    }

    /// <summary>Pixel size the shell thumbnail is fetched at; tiles scale this down to the
    /// slider's current size, so one fetch serves every zoom level. Set to 2× the largest tile
    /// (256) so it stays crisp when downscaled — and, crucially, isn't upscaled on high-DPI
    /// displays where a 256-tile can render at 384–512 physical pixels.</summary>
    private const int ThumbnailPixelSize = 512;

    private ImageSource? _thumbnail;
    private bool _thumbnailRequested;

    /// <summary>A large Explorer-style thumbnail, loaded lazily off the UI thread the first
    /// time a tile asks for it (only realized tiles do, thanks to virtualization). Shows the
    /// small shell icon until the real thumbnail arrives, then falls back to it on failure.</summary>
    public ImageSource? Thumbnail
    {
        get
        {
            if (!_thumbnailRequested)
            {
                _thumbnailRequested = true;
                _thumbnail = Icon; // instant placeholder while the real one loads
                _ = LoadThumbnailAsync();
            }
            return _thumbnail;
        }
    }

    private async Task LoadThumbnailAsync()
    {
        var image = await Task.Run(() => ShellThumbnails.GetThumbnail(FullPath, ThumbnailPixelSize));
        if (image is null) return; // keep the icon placeholder
        _thumbnail = image;
        OnPropertyChanged(nameof(Thumbnail));
    }

    public string SizeDisplay =>
        SizeBytes is { } b
            ? ByteSizeFormatter.Format(b) + (SizeIncomplete ? " *" : "")
            : IsDirectory ? "—" : "";

    public string ModifiedDisplay => ModifiedUtc == default ? "" : ModifiedUtc.ToLocalTime().ToString("g");

    /// <summary>Unknown sizes sort together at the small end.</summary>
    public long SizeSortKey => SizeBytes ?? -1;

    public string SizeTooltip =>
        IsDirectory && SizeComputedUtc is { } computed
            ? $"Computed {computed.ToLocalTime():g}" + (SizeIncomplete ? "\nMay be incomplete — some folders were inaccessible." : "")
            : SizeDisplay;
}
