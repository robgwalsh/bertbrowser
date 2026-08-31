using System.Collections.ObjectModel;
using System.Windows.Threading;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.ViewModels;

public sealed partial class FileListViewModel : ObservableObject
{
    private readonly IFileSystemService _fileSystem;
    private readonly DirSizeRepository _dirSizeRepository;

    // Read directly rather than through the shell, like the tab's IncludeHidden: a file list is
    // per tab and per pane, and nothing global should have to reach down into it.
    private readonly AppSettings _settings;

    private readonly BertBrowser.Core.Services.Archives.IArchiveBrowser _archives;

    [ObservableProperty]
    private bool _isLoading;

    [ObservableProperty]
    private bool _isFlattened;

    // --- Columns ---

    private IReadOnlyList<ColumnSetting>? _columnLayout;

    /// <summary>
    /// The columns this list shows, in order. Null means the tab has never been given one and takes
    /// whatever <see cref="ColumnLayoutRules.Resolve"/> makes of that.
    /// </summary>
    /// <remarks>
    /// Per list rather than global, the way <c>ShowPreviewPane</c> is per tab: a column set is
    /// something you arrange for the folder you are looking at.
    /// </remarks>
    public IReadOnlyList<ColumnSetting>? ColumnLayout
    {
        get => _columnLayout;
        set
        {
            _columnLayout = value;
            AttachHydrator();
            // Anything assigning this is a person arranging columns: the header menu, a header drag,
            // a gripper. ApplyDefaultColumns is the one path that is not, and it says so.
            ColumnsCustomized = true;
            ColumnsChanged?.Invoke(this, EventArgs.Empty);
        }
    }

    /// <summary>
    /// Whether this list's columns were arranged here rather than inherited from the saved default.
    /// </summary>
    /// <remarks>
    /// What stops a new default, saved in Settings, from wiping out an arrangement someone made in a
    /// tab that is still open — and equally, what lets that new default reach every tab that has not
    /// been touched, so the settings page is not a control that appears to do nothing.
    /// </remarks>
    public bool ColumnsCustomized { get; private set; }

    /// <summary>Takes a new saved default without claiming the tab was customised.</summary>
    public void ApplyDefaultColumns(IReadOnlyList<ColumnSetting>? layout)
    {
        if (ColumnsCustomized) return;
        _columnLayout = layout;
        AttachHydrator();
        ColumnsChanged?.Invoke(this, EventArgs.Empty);
    }

    /// <summary>Restores a layout saved with the session, which was customised when it was saved and
    /// must stay that way — otherwise the next settings change would silently discard it.</summary>
    public void RestoreColumns(IReadOnlyList<ColumnSetting>? layout)
    {
        if (layout is null) return;
        ColumnLayout = layout;
    }

    /// <summary>
    /// The columns to build, with Folder and Match put in or left out according to what this list is
    /// currently showing.
    /// </summary>
    public IReadOnlyList<ResolvedColumn> ResolvedColumns =>
        ColumnLayoutRules.Resolve(ColumnLayout, IsFlattened, ShowsContentMatches);

    /// <summary>Fills the shell-metadata cells. Created lazily, so a list that never shows such a
    /// column never builds one.</summary>
    private ShellMetadataHydrator? _hydrator;

    /// <summary>What the view has actually realized. Supplied by <c>DirectoryTabView</c>, which is
    /// the only thing that can know; without it a fast scroll would queue a file open for every row
    /// that flew past.</summary>
    public Func<IReadOnlyCollection<FileItemViewModel>>? RealizedRows { get; set; }

    internal ShellMetadataHydrator Hydrator
    {
        get
        {
            if (_hydrator is not null) return _hydrator;
            _hydrator = new ShellMetadataHydrator(Dispatcher.CurrentDispatcher)
            {
                RealizedRows = RealizedRows,
            };
            _hydrator.Idle += (_, _) => OnPropertyChanged(nameof(IsHydratingMetadata));
            return _hydrator;
        }
    }

    /// <summary>Whether metadata is still arriving. <c>UiSession.Settle</c> waits on this, so a
    /// scripted capture cannot photograph half-filled columns and no script needs a sleep.</summary>
    public bool IsHydratingMetadata => _hydrator?.IsBusy == true;

    /// <summary>Points the hydrator at the columns now in force and hands every row a way to reach
    /// it. Called whenever the column set or the item collection changes.</summary>
    private void AttachHydrator()
    {
        var keys = ResolvedColumns
            .Where(c => c.Spec.Kind == ColumnKind.ShellProperty)
            .Select(c => c.Id)
            .ToList();

        if (keys.Count == 0 && _hydrator is null) return;

        Hydrator.SetColumns(keys);
        foreach (var item in Items)
            item.Hydrator = _hydrator;
        OnPropertyChanged(nameof(IsHydratingMetadata));
    }

    /// <summary>
    /// Raised when <see cref="ResolvedColumns"/> would come back different. The view rebuilds from
    /// it; there is no bindable path, because <c>GridView.Columns</c> has no <c>ItemsSource</c>.
    /// </summary>
    public event EventHandler? ColumnsChanged;

    /// <summary>
    /// This listing is the inside of an archive rather than a folder on disk.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The guard the context menu and the key handler consult, exactly as they consult
    /// <see cref="IsFlattened"/>. Everything that writes to disk by path is off while it is true,
    /// because an entry's path is not one any executor can act on.
    /// </para>
    /// <para>
    /// Set from a real <c>File.Exists</c> on the worker thread, never from
    /// <c>ArchivePath.LooksVirtual</c>: a genuine folder named <c>photos.zip</c> must not put the
    /// list into archive mode, and only asking the disk can tell the two apart.
    /// </para>
    /// </remarks>
    [ObservableProperty]
    private bool _isInsideArchive;

    /// <summary>
    /// The listing failed because the container needs a password nobody has given.
    /// </summary>
    /// <remarks>
    /// A state rather than an event, which is what lets the banner carry an Unlock button: a modal
    /// raised from the background load that discovered this would be raised off the UI thread, and
    /// would have to be raised again on every back, forward and refresh.
    /// </remarks>
    [ObservableProperty]
    private bool _isArchiveLocked;

    [ObservableProperty]
    private string? _errorMessage;

    /// <summary>Shown centered in the file panel when a search finishes with no hits.</summary>
    [ObservableProperty]
    private string? _emptyMessage;

    /// <summary>
    /// The id of the column the list is sorted by — a <see cref="ColumnCatalog"/> id, so a shell
    /// property can be sorted by as readily as a built-in.
    /// </summary>
    /// <remarks>
    /// A string rather than an enum, and the built-in ids are exactly the members the enum used to
    /// have: <c>SessionTab.SortBy</c> has always persisted this as a string with a documented
    /// fallback, so every settings file written by every previous build keeps working untouched.
    /// Always assigned through <see cref="SetSort"/>, which normalises through the catalogue — an
    /// unusable id must degrade to Name rather than reach <see cref="CurrentComparer"/>.
    /// </remarks>
    [ObservableProperty]
    private string _sortBy = ColumnCatalog.Name;

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
        IFileSystemService fileSystem, DirSizeRepository dirSizeRepository, AppSettings settings,
        BertBrowser.Core.Services.Archives.IArchiveBrowser archives)
    {
        _fileSystem = fileSystem;
        _dirSizeRepository = dirSizeRepository;
        _settings = settings;
        _archives = archives;
        _tileAspect = AspectRatio.Parse(settings.TileAspectRatio);
        // The field, not the property: seeding is not customising, and going through the setter
        // would make every new tab claim an arrangement nobody made.
        _columnLayout = settings.FileListColumns;
    }

    /// <summary>Normal browsing: direct children of <paramref name="path"/>.</summary>
    public async Task LoadDirectoryAsync(string path, bool includeHidden, CancellationToken ct)
    {
        IsLoading = true;
        IsFlattened = false;
        // Cleared here and not only set by a search: this was the one assignment site, so clearing
        // a content: search left the flag true and its Match column on screen over an ordinary
        // directory listing, with nothing in it and nothing that would ever take it away again.
        ShowsContentMatches = false;
        ErrorMessage = null;
        EmptyMessage = null;
        IsArchiveLocked = false;
        try
        {
            // Resolved first, and off the UI thread. First because a damaged archive makes the
            // listing throw, and the guards still have to know where they are — a banner about a
            // broken container with Rename and Delete live over it would be worse than useless.
            // A real File.Exists, not LooksVirtual: a genuine folder named "photos.zip" must not
            // put the list into a mode where writing is switched off.
            var insideArchive = await Task.Run(() => _archives.Resolve(path) is not null, ct);
            ct.ThrowIfCancellationRequested();
            IsInsideArchive = insideArchive;

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
        catch (BertBrowser.Core.Services.Archives.ArchiveLockedException ex)
        {
            // Before the IOException arm, which would otherwise swallow it: this is the one listing
            // failure that has something the user can do about it.
            Items.Clear();
            ErrorMessage = ex.Message;
            IsArchiveLocked = true;
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

    /// <summary>
    /// Whether this result came from a search that read file contents.
    /// </summary>
    /// <remarks>
    /// <strong>Deliberately not <see cref="IsFlattened"/>, which every search sets.</strong> The
    /// Match column has nothing to show for an ordinary search, and an empty column that appears
    /// whenever you type reads as a rendering fault rather than as a feature.
    /// </remarks>
    [ObservableProperty]
    private bool _showsContentMatches;

    /// <summary>Search mode: prepares an empty flattened list for streamed hits to append into.</summary>
    public void BeginSearch()
    {
        IsLoading = true;
        IsFlattened = true;
        ErrorMessage = null;
        EmptyMessage = null;
        Items = new ObservableCollection<FileItemViewModel>();
    }

    /// <summary>
    /// Ends a search that was stopped rather than completed, keeping the rows it had already found.
    /// </summary>
    /// <remarks>
    /// <see cref="BeginSearch"/> raises <see cref="IsLoading"/> and only
    /// <see cref="CompleteSearchAsync"/>'s <c>finally</c> ever lowered it. That was invisible while
    /// every cancel was immediately followed by another load — but a stop that keeps its results
    /// has no such load, so without this the progress bar spins for ever and the harness's
    /// <c>Settle</c>, which waits on exactly this flag, never returns.
    /// </remarks>
    public void EndSearch(string? message = null)
    {
        IsLoading = false;
        if (message is not null && Items.Count == 0) EmptyMessage = message;
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
            hit.RelativeDirDisplay,
            hit.Match);

    /// <summary>Sorts by a column, or reverses the direction if it is already the one in force.</summary>
    /// <remarks>
    /// Normalised through the catalogue on the way in, so an id from a hand-edited settings file, a
    /// newer build, or an unsortable column (Match) becomes Name here rather than reaching the
    /// comparer.
    /// </remarks>
    public void SetSort(string columnId)
    {
        var column = ColumnCatalog.SortSpec(columnId).Id;
        if (string.Equals(SortBy, column, StringComparison.OrdinalIgnoreCase))
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

    /// <remarks>
    /// A folder the lister already sized exactly is skipped, not merely missed. Its path is not a
    /// real one — that is the only way a lister knows a recursive total up front — so asking
    /// <c>dir_size_cache</c> about it is a round trip that can only come back empty, and a hit
    /// would be a row that should never have been written.
    /// </remarks>
    private async Task HydrateDirSizesAsync(IReadOnlyList<FileItemViewModel> items, CancellationToken ct)
    {
        var dirs = items.Where(i => i is { IsDirectory: true, SizeBytes: null }).ToList();
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

    private void ReplaceItems(IReadOnlyList<FileItemViewModel> items)
    {
        Items = new ObservableCollection<FileItemViewModel>(items);
        // Every row needs the way back to the hydrator, and a new listing abandons whatever the
        // last one had in flight — those reads are for rows that are gone.
        _hydrator?.Reset();
        AttachHydrator();
    }

    /// <summary>
    /// Orders a list in place.
    /// </summary>
    /// <remarks>
    /// <b>The metadata values are snapshotted first, and that is not tidiness.</b>
    /// <see cref="List{T}.Sort(Comparison{T})"/> throws
    /// <c>InvalidOperationException: IComparer.Compare() method returns inconsistent results</c> if
    /// the comparer ever changes its mind mid-sort — and a comparer reading a cache that background
    /// hydration is still filling will eventually do exactly that. Reading everything once, up
    /// front, makes the comparison a pure function of a fixed table. The failure it prevents is a
    /// crash that only appears under load, on a big folder, in someone else's session.
    /// </remarks>
    private void SortInPlace(List<FileItemViewModel> items)
    {
        var snapshot = SnapshotSortValues(items);
        items.Sort(CurrentComparer(snapshot));
    }

    /// <summary>The metadata values as they stand right now, or null when the sort is by a column
    /// the rows answer themselves.</summary>
    private Dictionary<FileItemViewModel, ColumnValue?>? SnapshotSortValues(
        IReadOnlyList<FileItemViewModel> items)
    {
        if (ColumnCatalog.SortSpec(SortBy) is not { Kind: ColumnKind.ShellProperty } spec) return null;

        var snapshot = new Dictionary<FileItemViewModel, ColumnValue?>(items.Count);
        foreach (var item in items)
            snapshot[item] = item.ColumnValueFor(spec.Id);
        return snapshot;
    }

    /// <summary>
    /// The order the list is showing. Shared with the live-refresh merge, which has to place an
    /// arriving row — a second implementation there would drift from this one and put new files in
    /// the wrong place under any sort but the default.
    /// </summary>
    private Comparison<FileItemViewModel> CurrentComparer(
        Dictionary<FileItemViewModel, ColumnValue?>? metadata = null)
    {
        var spec = ColumnCatalog.SortSpec(SortBy);

        // A metadata column sorts on what has been read and nothing else — the snapshot, never a
        // live lookup. Rows whose value has not arrived have no rank, and the band below keeps them
        // at the bottom in *both* directions: a blank is the absence of a value, not a small one.
        if (spec.Kind == ColumnKind.ShellProperty)
        {
            var values = metadata ?? [];
            return (a, b) =>
            {
                var band = LayoutBand(a) - LayoutBand(b);
                if (band != 0) return band;

                values.TryGetValue(a, out var left);
                values.TryGetValue(b, out var right);

                var known = ColumnComparison.KnownBand(left) - ColumnComparison.KnownBand(right);
                if (known != 0) return known;

                var ordered = ColumnComparison.Compare(left, right, NaturalStringComparer.Instance);
                if (ordered == 0) ordered = NaturalStringComparer.Instance.Compare(a.Name, b.Name);
                return SortDescending ? -ordered : ordered;
            };
        }

        Comparison<FileItemViewModel> cmp = spec.Id switch
        {
            ColumnCatalog.Size => (a, b) => a.SizeSortKey.CompareTo(b.SizeSortKey),
            ColumnCatalog.Type => (a, b) => string.Compare(a.TypeName, b.TypeName, StringComparison.OrdinalIgnoreCase),
            ColumnCatalog.Modified => (a, b) => a.ModifiedUtc.CompareTo(b.ModifiedUtc),
            ColumnCatalog.Created => (a, b) => a.CreatedUtc.CompareTo(b.CreatedUtc),
            ColumnCatalog.Accessed => (a, b) => a.AccessedUtc.CompareTo(b.AccessedUtc),
            ColumnCatalog.Attributes => (a, b) =>
                string.Compare(a.AttributesDisplay, b.AttributesDisplay, StringComparison.Ordinal),
            ColumnCatalog.Extension => (a, b) =>
                string.Compare(a.ExtensionDisplay, b.ExtensionDisplay, StringComparison.OrdinalIgnoreCase),
            ColumnCatalog.RelativePath => (a, b) => NaturalStringComparer.Instance.Compare(a.RelativePath, b.RelativePath),
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
