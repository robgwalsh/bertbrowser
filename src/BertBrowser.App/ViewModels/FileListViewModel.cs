using System.Collections.ObjectModel;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public enum SortColumn
{
    Name,
    Size,
    Type,
    Modified,
    RelativePath,
}

public sealed partial class FileListViewModel : ObservableObject
{
    private readonly IFileSystemService _fileSystem;
    private readonly DirSizeRepository _dirSizeRepository;

    // Read directly rather than through the shell, like the tab's IncludeHidden: a file list is
    // per tab and per pane, and nothing global should have to reach down into it.
    private readonly AppSettings _settings;

    [ObservableProperty]
    private bool _isLoading;

    [ObservableProperty]
    private bool _isFlattened;

    [ObservableProperty]
    private string? _errorMessage;

    /// <summary>Shown centered in the file panel when a search finishes with no hits.</summary>
    [ObservableProperty]
    private string? _emptyMessage;

    [ObservableProperty]
    private SortColumn _sortBy = SortColumn.Name;

    [ObservableProperty]
    private bool _sortDescending;

    // Thumbnail zoom. The footer slider drives ThumbnailScale (0..1). 0 keeps the details
    // list; anything above switches to thumbnail tiles whose pixel size ramps from
    // MinThumbnail to MaxThumbnail. A small dead-zone just above 0 snaps to the minimum size
    // so the user doesn't have to land on an exact pixel to get the smallest thumbnails.
    private const double MinThumbnail = 64;
    private const double MaxThumbnail = 256;
    private const double DeadZone = 0.05;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ThumbnailSize), nameof(ThumbnailTileHeight), nameof(IsThumbnailView))]
    private double _thumbnailScale;

    /// <summary>Effective tile width in pixels (0 = details list).</summary>
    public double ThumbnailSize
    {
        get
        {
            if (ThumbnailScale <= 0) return 0;
            if (ThumbnailScale < DeadZone) return MinThumbnail;
            var t = (ThumbnailScale - DeadZone) / (1 - DeadZone);
            return MinThumbnail + t * (MaxThumbnail - MinThumbnail);
        }
    }

    /// <summary>Tile height for the current width, from the configured aspect ratio. The width is
    /// what the zoom slider drives, so a taller ratio grows the tile downwards rather than
    /// shrinking it — the slider keeps meaning the same thing at every shape.</summary>
    public double ThumbnailTileHeight => _tileAspect.HeightFor(ThumbnailSize);

    /// <summary>Parsed once and cached: this is read by every visible tile's height binding.</summary>
    private AspectRatio _tileAspect;

    /// <summary>Re-reads the tile aspect ratio after the Settings dialog commits one. Every open
    /// list needs telling — the setting is global, and the shell fans it out.</summary>
    public void RefreshTileAspect()
    {
        var aspect = AspectRatio.Parse(_settings.TileAspectRatio);
        if (aspect == _tileAspect) return;
        _tileAspect = aspect;
        OnPropertyChanged(nameof(ThumbnailTileHeight));
    }

    public bool IsThumbnailView => ThumbnailSize > 0;

    private bool _lastThumbnailView;

    partial void OnThumbnailScaleChanged(double value)
    {
        // Crossing the details/thumbnail boundary changes the item bands (media move to the
        // bottom), so re-sort in place; resizing within thumbnail mode needs no reshuffle.
        if (IsThumbnailView == _lastThumbnailView) return;
        _lastThumbnailView = IsThumbnailView;

        if (Items.Count == 0) return; // a fresh load will sort with the right mode anyway
        var items = Items.ToList();
        SortInPlace(items);
        ReplaceItems(items);
    }

    [ObservableProperty]
    private ObservableCollection<FileItemViewModel> _items = new();

    public FileListViewModel(
        IFileSystemService fileSystem, DirSizeRepository dirSizeRepository, AppSettings settings)
    {
        _fileSystem = fileSystem;
        _dirSizeRepository = dirSizeRepository;
        _settings = settings;
        _tileAspect = AspectRatio.Parse(settings.TileAspectRatio);
    }

    /// <summary>Normal browsing: direct children of <paramref name="path"/>.</summary>
    public async Task LoadDirectoryAsync(string path, bool includeHidden, CancellationToken ct)
    {
        IsLoading = true;
        IsFlattened = false;
        ErrorMessage = null;
        EmptyMessage = null;
        try
        {
            var entries = await Task.Run(() => _fileSystem.ListDirectory(path), ct);
            ct.ThrowIfCancellationRequested();

            var items = await Task.Run(() =>
            {
                var vms = entries
                    .Where(e => includeHidden || !e.Attributes.HasFlag(FileAttributes.Hidden))
                    .Select(e => new FileItemViewModel(e))
                    .ToList();
                SortInPlace(vms);
                return vms;
            }, ct);

            ReplaceItems(items);

            await HydrateDirSizesAsync(items, ct);
        }
        catch (OperationCanceledException)
        {
            // superseded by a newer navigation
        }
        catch (UnauthorizedAccessException)
        {
            Items.Clear();
            ErrorMessage = AccessDeniedMessage(path);
        }
        catch (IOException ex)
        {
            Items.Clear();
            ErrorMessage = ex.Message;
        }
        finally
        {
            IsLoading = false;
        }
    }

    /// <summary>
    /// Brings the list up to date with what is on disk, touching only the rows that actually
    /// changed — what a watcher-driven refresh uses instead of <see cref="LoadDirectoryAsync"/>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The difference that matters is that <see cref="Items"/> is never replaced. The view focuses
    /// the file list whenever that collection changes, and a load clears the selection with it —
    /// right for a navigation the user asked for, wrong for a refresh nobody asked for. Merging
    /// leaves untouched rows as the very same objects, so the selection, the scroll position and
    /// the keyboard focus all survive a file appearing on disk.
    /// </para>
    /// <para>
    /// <see cref="IsLoading"/> is deliberately not set: it drives the progress bar and the harness's
    /// idea of quiescence, and a background refresh is not a load anyone is waiting for.
    /// </para>
    /// </remarks>
    public async Task<bool> MergeDirectoryAsync(string path, bool includeHidden, CancellationToken ct)
    {
        // A flattened search result is not a folder listing, and an errored list has nothing to
        // merge into — both want a real reload if they want anything.
        if (IsFlattened || ErrorMessage is not null) return false;

        try
        {
            var entries = await Task.Run(
                () => _fileSystem.ListDirectory(path)
                    .Where(e => includeHidden || !e.Attributes.HasFlag(FileAttributes.Hidden))
                    .ToList(),
                ct);
            ct.ThrowIfCancellationRequested();

            var current = Items.Select(i => i.ToEntry()).ToList();
            var changes = await Task.Run(() => FileListDiff.Compute(current, entries), ct);
            ct.ThrowIfCancellationRequested();

            if (!changes.Any) return true;

            ApplyChanges(changes);
            EmptyMessage = null;

            // Only the folders that arrived need a size; the ones already listed keep theirs.
            await HydrateDirSizesAsync(
                changes.Added.Where(e => e.IsDirectory)
                    .Select(e => Items.FirstOrDefault(i => i.FullPath == e.FullPath))
                    .OfType<FileItemViewModel>()
                    .ToList(),
                ct);

            return true;
        }
        catch (OperationCanceledException)
        {
            return false;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // The folder went, or closed to us. That is a navigation problem, not a refresh one —
            // the caller decides what to do about it.
            return false;
        }
    }

    private void ApplyChanges(FileListChanges changes)
    {
        foreach (var key in changes.Removed)
        {
            var index = IndexOfKey(key);
            if (index >= 0) Items.RemoveAt(index);
        }

        // Replaced rather than mutated in place: a FileItemViewModel takes its values in its
        // constructor, and one row swapping is far cheaper than the whole collection doing so.
        foreach (var entry in changes.Updated)
        {
            var index = IndexOfKey(PathKey.Canonicalize(entry.FullPath));
            if (index >= 0) Items[index] = new FileItemViewModel(entry);
        }

        foreach (var entry in changes.Added)
            Items.Insert(InsertionPointFor(entry), new FileItemViewModel(entry));
    }

    private int IndexOfKey(string key)
    {
        for (var i = 0; i < Items.Count; i++)
        {
            if (PathKey.Canonicalize(Items[i].FullPath).Equals(key, StringComparison.Ordinal))
                return i;
        }
        return -1;
    }

    /// <summary>Where a new row belongs under the sort that is showing, so an arriving file lands
    /// in place rather than at the bottom until the next reload.</summary>
    private int InsertionPointFor(FileEntry entry)
    {
        var candidate = new FileItemViewModel(entry);
        var comparer = CurrentComparer();

        for (var i = 0; i < Items.Count; i++)
        {
            if (comparer(candidate, Items[i]) < 0) return i;
        }
        return Items.Count;
    }

    /// <summary>
    /// What to say about a folder Windows will not open for us.
    /// </summary>
    /// <remarks>
    /// This is a message people will actually see now. BertBrowser used to run with an
    /// administrator token and could list almost anything; it runs as the person using it, so
    /// <c>C:\System Volume Information</c>, another account's profile and similar folders are
    /// closed to it exactly as they are to Explorer. The bare framework message ("Access to the
    /// path … is denied.") reads like a defect, so say what it is and what would change it.
    /// </remarks>
    private static string AccessDeniedMessage(string path) =>
        $"Access is denied. \"{path}\" needs administrator rights, which BertBrowser does not " +
        "use — the same folders are closed to File Explorer.";

    /// <summary>Search mode: prepares an empty flattened list for streamed hits to append into.</summary>
    public void BeginSearch()
    {
        IsLoading = true;
        IsFlattened = true;
        ErrorMessage = null;
        EmptyMessage = null;
        Items = new ObservableCollection<FileItemViewModel>();
    }

    /// <summary>Appends one batch of live-scan hits (called on the UI thread via IProgress).</summary>
    public void AppendSearchHits(IReadOnlyList<SearchHit> hits)
    {
        foreach (var hit in hits)
            Items.Add(CreateSearchItem(hit));
    }

    /// <summary>Replaces the streamed list with the final sorted outcome and hydrates sizes.
    /// When <paramref name="hydrateMetadata"/> is set (global/MFT results, which carry no size or
    /// timestamp), each hit is stat'd from disk off-thread before sorting and binding.</summary>
    public async Task CompleteSearchAsync(SearchOutcome outcome, string queryText, bool hydrateMetadata, CancellationToken ct)
    {
        try
        {
            var items = await Task.Run(() =>
            {
                var vms = outcome.Hits.Select(CreateSearchItem).ToList();
                if (hydrateMetadata)
                    foreach (var vm in vms)
                        vm.HydrateSearchMetadata();
                SortInPlace(vms);
                return vms;
            }, ct);

            ReplaceItems(items);
            EmptyMessage = items.Count == 0 ? $"No results for '{queryText}'" : null;
            await HydrateDirSizesAsync(items, ct);
        }
        catch (OperationCanceledException)
        {
        }
        finally
        {
            IsLoading = false;
        }
    }

    /// <summary>Turns an index hit into a list row. Internal because the disk-usage view builds
    /// its "largest files" list from the same shape and must not grow a second version of this.</summary>
    internal static FileItemViewModel CreateSearchItem(SearchHit hit) =>
        new(new FileEntry(hit.Name, hit.DisplayPath, hit.IsDirectory,
                hit.IsDirectory ? -1 : hit.SizeBytes, hit.ModifiedUtc,
                hit.Hidden ? FileAttributes.Hidden : 0),
            hit.RelativeDirDisplay);

    public void SetSort(SortColumn column)
    {
        if (SortBy == column)
        {
            SortDescending = !SortDescending;
        }
        else
        {
            SortBy = column;
            SortDescending = false;
        }
        var items = Items.ToList();
        SortInPlace(items);
        ReplaceItems(items);
    }

    /// <summary>Re-reads cached directory sizes for the current normal-mode listing.</summary>
    public async Task RefreshDirSizesAsync(CancellationToken ct)
    {
        if (!IsFlattened)
            await HydrateDirSizesAsync(Items.ToList(), ct);
    }

    private async Task HydrateDirSizesAsync(IReadOnlyList<FileItemViewModel> items, CancellationToken ct)
    {
        var dirs = items.Where(i => i.IsDirectory).ToList();
        if (dirs.Count == 0) return;

        var cache = await Task.Run(
            () => _dirSizeRepository.GetMany(dirs.Select(d => d.FullPath).ToList()), ct);

        foreach (var dir in dirs)
        {
            if (cache.TryGetValue(PathKey.Canonicalize(dir.FullPath), out var result))
            {
                dir.SizeBytes = result.SizeBytes;
                dir.SizeIncomplete = result.Incomplete;
                dir.SizeComputedUtc = result.ComputedUtc;
            }
        }
    }

    private void ReplaceItems(IReadOnlyList<FileItemViewModel> items) =>
        Items = new ObservableCollection<FileItemViewModel>(items);

    private void SortInPlace(List<FileItemViewModel> items) => items.Sort(CurrentComparer());

    /// <summary>
    /// The order the list is showing. Shared with the live-refresh merge, which has to place an
    /// arriving row — a second implementation there would drift from this one and put new files in
    /// the wrong place under any sort but the default.
    /// </summary>
    private Comparison<FileItemViewModel> CurrentComparer()
    {
        Comparison<FileItemViewModel> cmp = SortBy switch
        {
            SortColumn.Size => (a, b) => a.SizeSortKey.CompareTo(b.SizeSortKey),
            SortColumn.Type => (a, b) => string.Compare(a.TypeName, b.TypeName, StringComparison.OrdinalIgnoreCase),
            SortColumn.Modified => (a, b) => a.ModifiedUtc.CompareTo(b.ModifiedUtc),
            SortColumn.RelativePath => (a, b) => NaturalStringComparer.Instance.Compare(a.RelativePath, b.RelativePath),
            _ => (a, b) => NaturalStringComparer.Instance.Compare(a.Name, b.Name),
        };

        return (a, b) =>
        {
            // Layout bands (folders, then non-media files as rows, then media tiles) always
            // sort ahead of the column direction, so rows stay grouped above the thumbnails.
            var band = LayoutBand(a) - LayoutBand(b);
            if (band != 0) return band;

            var result = cmp(a, b);
            if (result == 0)
                result = NaturalStringComparer.Instance.Compare(a.Name, b.Name);
            return SortDescending ? -result : result;
        };
    }

    /// <summary>Ordering band: directories (0) first, then non-media files (1), then — only
    /// in thumbnail mode — media files (2) so they collect below the rows.</summary>
    private int LayoutBand(FileItemViewModel item)
    {
        if (!IsFlattened && item.IsDirectory) return 0;
        if (IsThumbnailView && item.IsMedia) return 2;
        return 1;
    }
}
