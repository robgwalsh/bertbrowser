using System.Windows.Media;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Compare;

namespace BertBrowser.App.ViewModels;

public sealed partial class FileItemViewModel : ObservableObject
{
    public string Name { get; }
    public string FullPath { get; }
    public bool IsDirectory { get; }
    public DateTime ModifiedUtc { get; private set; }
    public string TypeName { get; }

    /// <summary>
    /// The whole attribute set, not just the one bit a row used to render.
    /// </summary>
    /// <remarks>
    /// Kept in full so <see cref="ToEntry"/> round-trips faithfully. That is what
    /// <c>FileListDiff.Differs</c> rests on: while a rebuilt row could only reconstruct Hidden, a
    /// full comparison would have called every row different on every pass, so only Hidden could be
    /// weighed — and an Attributes column then had nothing honest to show.
    /// <para>
    /// Thin on a search row, which knows only Hidden (<c>SearchHit</c> carries a bool). That cannot
    /// bite the merge, which refuses a flattened list outright.
    /// </para>
    /// </remarks>
    public FileAttributes Attributes { get; private set; }

    /// <summary>When it was created, and <see cref="AccessedUtc"/> when it was last read.
    /// <c>default</c> means unknown — a search hit and an archive entry have neither.</summary>
    public DateTime CreatedUtc { get; private set; }

    public DateTime AccessedUtc { get; private set; }

    /// <summary>Hidden (own or inherited) — drives the dimmed-icon treatment.</summary>
    public bool IsHidden => Attributes.HasFlag(FileAttributes.Hidden);

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

    // --- Shell-metadata columns ---

    /// <summary>Set by the list that owns this row; null until then, and null forever for a row in
    /// a list with no metadata columns on it.</summary>
    internal ShellMetadataHydrator? Hydrator { get; set; }

    /// <summary>
    /// What this row shows in a shell-metadata column, by canonical name. The binding behind every
    /// such cell is <c>{Binding [System.Image.Dimensions]}</c>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Reading one is a cache lookup and, on a miss, a note asking for it — never a file open on the
    /// UI thread. There is deliberately no dictionary per row: a folder can hold two hundred
    /// thousand rows while the user looks at forty, so this costs nothing for a row nobody ever
    /// scrolls to.
    /// </para>
    /// <para>
    /// <b>The cell is refreshed by raising <see cref="Columns"/>, not <c>"Item[]"</c>.</b> The
    /// indexer lives on the struct rather than on the row, so <c>"Item[]"</c> from the row is a
    /// notification about an indexer the binding is not using and WPF ignores it — the values
    /// arrived, were cached, and the column stayed blank for ever. Naming the property makes WPF
    /// re-read it and re-evaluate the indexer behind it. Nothing in C# can see the difference, which
    /// is why <c>assert-metadata</c> reads the rendered cell rather than asking the row.
    /// </para>
    /// </remarks>
    public ShellColumnValues Columns => _columns ??= new ShellColumnValues(this);

    private ShellColumnValues? _columns;

    /// <summary>
    /// Values have arrived; re-read the cells that were waiting for them.
    /// </summary>
    /// <remarks>
    /// <b>A fresh instance, not just a notification.</b> This was a <c>readonly struct</c> holding
    /// only the row, so re-reading <see cref="Columns"/> handed WPF a value equal to the one it
    /// already had, it concluded nothing had changed, and it never re-evaluated the indexer behind
    /// it — every metadata column rendered permanently blank while every value was read, cached and
    /// discarded. Replacing the object is what makes the change visible.
    /// </remarks>
    internal void NotifyColumnsChanged()
    {
        _columns = new ShellColumnValues(this);
        OnPropertyChanged(nameof(Columns));
    }

    /// <summary>The indexer behind a metadata cell. One small object per row that has values, not a
    /// dictionary: a folder can hold two hundred thousand rows and this costs nothing for the ones
    /// nobody scrolls to.</summary>
    public sealed class ShellColumnValues(FileItemViewModel row)
    {
        public string this[string canonical] =>
            row.Hydrator?.Value(row, canonical)?.Display ?? "";
    }

    /// <summary>The typed sort key for a metadata column, or null when it has not been read.
    /// Deliberately a <em>peek</em>: sorting compares every row against several others, so asking
    /// here would queue a file open for every row in the folder on one click of a header.</summary>
    internal ColumnValue? ColumnValueFor(string canonical) => Hydrator?.Peek(this, canonical);

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
        Attributes, CreatedUtc, AccessedUtc);

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay), nameof(SizeSortKey))]
    private long? _sizeBytes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay))]
    private bool _sizeIncomplete;

    [ObservableProperty]
    private DateTime? _sizeComputedUtc;

    /// <summary>
    /// How this row stands against the folder the other pane is showing, or
    /// <see cref="CompareRowState.None"/> when no comparison is running.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Stamped on by <c>FileListViewModel</c> as the row is built, never worked out here: a row is
    /// rebuilt whenever its file changes on disk, so it cannot be the thing that remembers. The
    /// comparison is.
    /// </para>
    /// <para>
    /// Deliberately outside <see cref="ToEntry"/>, and therefore outside what
    /// <c>FileListDiff.Differs</c> weighs. It is not a property of the file, and counting it would
    /// have every refresh call every row changed for as long as a comparison was on screen.
    /// </para>
    /// </remarks>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CompareStatusDisplay))]
    private CompareRowState _compareState;

    /// <summary>What the compare status column shows. Colour alone is not something everyone can
    /// read, and it is the one thing no contrast test can check.</summary>
    public string CompareStatusDisplay => CompareState switch
    {
        CompareRowState.OnlyHere => "Only here",
        CompareRowState.Newer => "Newer",
        CompareRowState.Older => "Older",
        CompareRowState.Differs => "Differs",
        CompareRowState.Same => "Same",
        CompareRowState.Unknown => "Not compared",
        _ => "",
    };

    private ImageSource? _icon;
    private bool _iconLoaded;
    private bool _iconLoading;

    public FileItemViewModel(FileEntry entry)
    {
        Name = entry.Name;
        FullPath = entry.FullPath;
        IsDirectory = entry.IsDirectory;
        ModifiedUtc = entry.ModifiedUtc;
        Attributes = entry.Attributes;
        CreatedUtc = entry.CreatedUtc;
        AccessedUtc = entry.AccessedUtc;
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
                CreatedUtc = info.CreationTimeUtc;
                AccessedUtc = info.LastAccessTimeUtc;
                Attributes = info.Attributes;
            }
            else
            {
                var info = new FileInfo(FullPath);
                if (!info.Exists) return;
                SizeBytes = info.Length;
                ModifiedUtc = info.LastWriteTimeUtc;
                CreatedUtc = info.CreationTimeUtc;
                AccessedUtc = info.LastAccessTimeUtc;
                Attributes = info.Attributes;
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

    public string ModifiedDisplay => Stamp(ModifiedUtc);

    public string CreatedDisplay => Stamp(CreatedUtc);

    public string AccessedDisplay => Stamp(AccessedUtc);

    /// <summary>A timestamp, or nothing at all when it is unknown. Blank rather than
    /// <c>01/01/0001</c>, which reads as data — an archive entry and an unhydrated search row both
    /// land here.</summary>
    private static string Stamp(DateTime utc) => utc == default ? "" : utc.ToLocalTime().ToString("g");

    /// <summary>Explorer's letters, in Explorer's order. Blank for a row that does not know its
    /// attributes rather than claiming the file has none.</summary>
    public string AttributesDisplay
    {
        get
        {
            if (Attributes == 0) return "";
            Span<char> flags = stackalloc char[7];
            var n = 0;
            if (Attributes.HasFlag(FileAttributes.ReadOnly)) flags[n++] = 'R';
            if (Attributes.HasFlag(FileAttributes.Hidden)) flags[n++] = 'H';
            if (Attributes.HasFlag(FileAttributes.System)) flags[n++] = 'S';
            if (Attributes.HasFlag(FileAttributes.Archive)) flags[n++] = 'A';
            if (Attributes.HasFlag(FileAttributes.Compressed)) flags[n++] = 'C';
            if (Attributes.HasFlag(FileAttributes.Encrypted)) flags[n++] = 'E';
            if (Attributes.HasFlag(FileAttributes.ReparsePoint)) flags[n++] = 'L';
            return new string(flags[..n]);
        }
    }

    /// <summary>The extension without its dot, as Explorer shows it. A folder has none.</summary>
    public string ExtensionDisplay =>
        IsDirectory || Path.GetExtension(Name) is not { Length: > 1 } ext ? "" : ext[1..];

    /// <summary>Unknown sizes sort together at the small end.</summary>
    public long SizeSortKey => SizeBytes ?? -1;

    public string SizeTooltip =>
        IsDirectory && SizeComputedUtc is { } computed
            ? $"Computed {computed.ToLocalTime():g}" + (SizeIncomplete ? "\nMay be incomplete — some folders were inaccessible." : "")
            : SizeDisplay;
}
