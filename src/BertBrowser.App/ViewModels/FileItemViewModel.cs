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
    public double IconOpacity => IsHidden ? 0.45 : 1.0;

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

    /// <summary>Path relative to the filter root; only set in flattened tag-filter mode.</summary>
    public string RelativePath { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay), nameof(SizeSortKey))]
    private long? _sizeBytes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay))]
    private bool _sizeIncomplete;

    [ObservableProperty]
    private bool _isSizeComputing;

    [ObservableProperty]
    private DateTime? _sizeComputedUtc;

    [ObservableProperty]
    private bool _isMissing;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(TagsDisplay), nameof(TagsSortKey))]
    private IReadOnlyList<Tag> _tags = Array.Empty<Tag>();

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
        SizeBytes = entry.IsDirectory ? null : entry.SizeBytes;
        TypeName = entry.IsDirectory
            ? "Folder"
            : Path.GetExtension(entry.Name) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    /// <summary>Search-result mode: a real filesystem entry (file or directory) plus its
    /// parent path relative to the search root.</summary>
    public FileItemViewModel(FileEntry entry, string relativePath) : this(entry)
    {
        RelativePath = relativePath;
    }

    public FileItemViewModel(string fullPath, string relativePath, IReadOnlyList<Tag> tags)
    {
        Name = Path.GetFileName(fullPath);
        FullPath = fullPath;
        IsDirectory = false;
        RelativePath = relativePath;
        Tags = tags;
        TypeName = Path.GetExtension(fullPath) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    /// <summary>Fills size/modified from disk in flattened mode; marks missing files.</summary>
    public void HydrateFromDisk()
    {
        var info = new FileInfo(FullPath);
        if (info.Exists)
        {
            SizeBytes = info.Length;
            ModifiedUtc = info.LastWriteTimeUtc;
            IsHidden = info.Attributes.HasFlag(FileAttributes.Hidden);
            OnPropertyChanged(nameof(ModifiedDisplay));
            OnPropertyChanged(nameof(IconOpacity));
        }
        else
        {
            IsMissing = true;
        }
    }

    /// <summary>Fills size/modified/hidden from disk for a search result whose index row
    /// lacked them — MFT-built rows carry no size or timestamp. Unlike
    /// <see cref="HydrateFromDisk"/> this never flags a directory as missing (it stats
    /// directories too); intended to run off the UI thread before the item is bound.</summary>
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

    public string TagsDisplay => string.Join(", ", Tags.Select(t => t.Name));

    /// <summary>Unknown sizes sort together at the small end.</summary>
    public long SizeSortKey => SizeBytes ?? -1;

    public string TagsSortKey =>
        Tags.Count == 0
            ? "￿" // untagged last
            : string.Join(",", Tags.Select(t => t.Name).OrderBy(n => n, StringComparer.OrdinalIgnoreCase));

    public string SizeTooltip =>
        IsDirectory && SizeComputedUtc is { } computed
            ? $"Computed {computed.ToLocalTime():g}" + (SizeIncomplete ? "\nMay be incomplete — some folders were inaccessible." : "")
            : SizeDisplay;
}
