using System.ComponentModel;
using System.Diagnostics;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed record BreadcrumbSegment(string Name, string FullPath);

/// <summary>
/// One browsable directory: its path, history, search state, and file list. This is the unit a tab
/// is made of — several exist at once, in one pane or across several, so everything here is
/// instance state and nothing reaches for "the" current directory.
/// </summary>
public sealed partial class DirectoryTabViewModel : ObservableObject, IDisposable
{
    private readonly ISearchService _searchService;
    private readonly IDirectorySizeService _sizeService;
    private readonly AppSettings _settings;

    private readonly Stack<string> _backStack = new();
    private readonly Stack<string> _forwardStack = new();
    private CancellationTokenSource _navigationCts = new();
    private CancellationTokenSource _searchDebounceCts = new();

    public FileListViewModel FileList { get; }

    /// <summary>Reflects the current "Show hidden items" setting (may change while running).
    /// Read straight from settings rather than pushed down from the shell: the toggle writes the
    /// setting before asking anything to refresh.</summary>
    private bool IncludeHidden => _settings.ShowHiddenItems;

    /// <summary>Raised after navigation so the view can select and scroll to a specific
    /// file (e.g. when a bookmarked file is opened).</summary>
    public event Action<string>? RevealFileRequested;

    /// <summary>Raised whenever this tab's location changes, so the shell can drive the folder
    /// tree from whichever tab is active without every tab watching every other.</summary>
    public event Action<DirectoryTabViewModel>? LocationChanged;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BreadcrumbSegments), nameof(CanGoUp), nameof(Title))]
    private string _currentPath = "";

    [ObservableProperty]
    private string _statusText = "Ready";

    /// <summary>"N items selected (size)" for the file-list selection; empty when nothing is
    /// selected. Kept beside <see cref="StatusText"/> so a selection never overwrites the
    /// navigation/search message.</summary>
    [ObservableProperty]
    private string _selectionSummary = "";

    [ObservableProperty]
    private string _searchText = "";

    /// <summary>Search scope: true = whole PC (the MFT global index), false = the current
    /// folder subtree. Defaults to whole-PC, the point of the MFT index.</summary>
    [ObservableProperty]
    private bool _searchGlobal = true;

    /// <summary>True while this is the visible tab of its pane. Gates work that must not happen
    /// for a background tab — most importantly stealing keyboard focus when a load completes.</summary>
    [ObservableProperty]
    private bool _isActive;

    /// <summary>The file list's live selection, mirrored out of the <c>ListView</c> so the shell
    /// and the window's key handlers never have to reach into a view to find it.</summary>
    public IReadOnlyList<FileItemViewModel> SelectedItems { get; internal set; } = [];

    /// <summary>Tab-strip caption: the folder's own name, or the drive for a root.</summary>
    public string Title
    {
        get
        {
            if (CurrentPath.Length == 0) return "New tab";
            var name = Path.GetFileName(CurrentPath.TrimEnd('\\'));
            return name.Length > 0 ? name : CurrentPath.TrimEnd('\\');
        }
    }

    public bool CanGoBack => _backStack.Count > 0;
    public bool CanGoForward => _forwardStack.Count > 0;
    public bool CanGoUp => CurrentPath.Length > 0 && Path.GetDirectoryName(CurrentPath) is not null;

    public IReadOnlyList<BreadcrumbSegment> BreadcrumbSegments
    {
        get
        {
            var segments = new List<BreadcrumbSegment>();
            if (CurrentPath.Length == 0) return segments;

            var root = Path.GetPathRoot(CurrentPath)!;
            segments.Add(new BreadcrumbSegment(root.TrimEnd('\\'), root));
            var rest = CurrentPath[root.Length..];
            var acc = root;
            foreach (var part in rest.Split('\\', StringSplitOptions.RemoveEmptyEntries))
            {
                acc = Path.Combine(acc, part);
                segments.Add(new BreadcrumbSegment(part, acc));
            }
            return segments;
        }
    }

    public DirectoryTabViewModel(
        IFileSystemService fileSystem,
        DirSizeRepository dirSizeRepository,
        ISearchService searchService,
        IDirectorySizeService sizeService,
        AppSettings settings)
    {
        _searchService = searchService;
        _sizeService = sizeService;
        _settings = settings;

        FileList = new FileListViewModel(fileSystem, dirSizeRepository);
        FileList.PropertyChanged += OnFileListPropertyChanged;
    }

    /// <summary>Cancels anything in flight and unsubscribes. A tab is closable, unlike the shell,
    /// so its subscriptions have to be given back.</summary>
    public void Dispose()
    {
        _navigationCts.Cancel();
        _searchDebounceCts.Cancel();
        FileList.PropertyChanged -= OnFileListPropertyChanged;
    }

    // --- Navigation ---

    [RelayCommand]
    public async Task NavigateToAsync(string path)
    {
        if (path.Equals(CurrentPath, StringComparison.OrdinalIgnoreCase)) return;
        if (!Directory.Exists(path))
        {
            StatusText = $"Folder not found: {path}";
            return;
        }

        path = ResolveInaccessibleJunction(path);
        if (path.Equals(CurrentPath, StringComparison.OrdinalIgnoreCase)) return;

        if (CurrentPath.Length > 0)
            _backStack.Push(CurrentPath);
        _forwardStack.Clear();

        await SetPathAndLoadAsync(path);
    }

    /// <summary>
    /// Windows' legacy compatibility junctions (<c>My Documents</c>, <c>Cookies</c>,
    /// <c>Application Data</c>, <c>Recent</c>, …) carry an explicit deny-list ACL on the
    /// reparse point itself so apps can't traverse the old shell path — listing them throws
    /// "Access is denied" even elevated. The deny is on the junction, not its target, so when a
    /// junction can't be listed directly we follow the reparse point to its real target (which
    /// <em>is</em> accessible) and browse there instead. Normal, listable junctions are left at
    /// their own path.
    /// </summary>
    private static string ResolveInaccessibleJunction(string path)
    {
        try
        {
            var info = new DirectoryInfo(path);
            if ((info.Attributes & FileAttributes.ReparsePoint) == 0)
                return path; // ordinary directory — nothing to follow

            try
            {
                using var probe = Directory.EnumerateFileSystemEntries(path).GetEnumerator();
                probe.MoveNext();
                return path; // listable junction — browse in place
            }
            catch (UnauthorizedAccessException)
            {
                return info.ResolveLinkTarget(returnFinalTarget: true)?.FullName ?? path;
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return path; // give up gracefully; the normal load path will report any error
        }
    }

    [RelayCommand(CanExecute = nameof(CanGoBack))]
    private async Task BackAsync()
    {
        _forwardStack.Push(CurrentPath);
        await SetPathAndLoadAsync(_backStack.Pop());
    }

    [RelayCommand(CanExecute = nameof(CanGoForward))]
    private async Task ForwardAsync()
    {
        _backStack.Push(CurrentPath);
        await SetPathAndLoadAsync(_forwardStack.Pop());
    }

    [RelayCommand(CanExecute = nameof(CanGoUp))]
    private async Task UpAsync()
    {
        var parent = Path.GetDirectoryName(CurrentPath);
        if (parent is not null)
            await NavigateToAsync(parent);
    }

    [RelayCommand]
    private async Task RefreshAsync() => await RefreshViewAsync();

    private async Task SetPathAndLoadAsync(string path)
    {
        ClearSearchState(); // navigating exits search mode, like Explorer
        CurrentPath = path;
        ApplyDirectoryThumbnailScale(path); // restore this folder's tile/list preference
        BackCommand.NotifyCanExecuteChanged();
        ForwardCommand.NotifyCanExecuteChanged();
        UpCommand.NotifyCanExecuteChanged();
        LocationChanged?.Invoke(this);
        await RefreshViewAsync();
    }

    public async Task RefreshViewAsync()
    {
        if (CurrentPath.Length == 0) return;

        _navigationCts.Cancel();
        _navigationCts = new CancellationTokenSource();
        var ct = _navigationCts.Token;

        try
        {
            if (SearchQuery.Parse(SearchText) is not null)
            {
                await RunSearchAsync(ct);
            }
            else
            {
                await FileList.LoadDirectoryAsync(CurrentPath, IncludeHidden, ct);
                if (!ct.IsCancellationRequested)
                    StatusText = $"{FileList.Items.Count} item(s)";
            }
        }
        catch (OperationCanceledException)
        {
        }
    }

    /// <summary>Reveals a file in its containing folder — navigating there first if needed.</summary>
    public async Task RevealFileAsync(string fullPath)
    {
        var parent = Path.GetDirectoryName(fullPath);
        if (parent is null) return;
        await NavigateToAsync(parent);
        RevealFileRequested?.Invoke(fullPath);
    }

    // --- Per-directory thumbnail zoom ---

    private bool _suppressThumbnailPersist;

    /// <summary>Persist the slider position for the directory the user changed it in, so tile
    /// vs. list (and the zoom level) is remembered per folder. Zero (details) drops the entry.</summary>
    private void OnFileListPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(FileListViewModel.ThumbnailScale)) return;
        if (_suppressThumbnailPersist || CurrentPath.Length == 0) return;

        var key = PathKey.Canonicalize(CurrentPath);
        if (FileList.ThumbnailScale > 0)
            _settings.DirectoryThumbnailScales[key] = FileList.ThumbnailScale;
        else
            _settings.DirectoryThumbnailScales.Remove(key);
    }

    /// <summary>Restores the saved zoom for <paramref name="path"/> (details if none) without
    /// counting the programmatic change as a user edit to persist.</summary>
    private void ApplyDirectoryThumbnailScale(string path)
    {
        var scale = _settings.DirectoryThumbnailScales.TryGetValue(PathKey.Canonicalize(path), out var s) ? s : 0;
        _suppressThumbnailPersist = true;
        FileList.ThumbnailScale = scale;
        _suppressThumbnailPersist = false;
    }

    // --- Search ---

    private bool _suppressSearchRefresh;

    partial void OnSearchTextChanged(string value)
    {
        _searchDebounceCts.Cancel();
        if (_suppressSearchRefresh) return;
        _searchDebounceCts = new CancellationTokenSource();
        _ = DebouncedSearchAsync(_searchDebounceCts.Token);
    }

    partial void OnSearchGlobalChanged(bool value)
    {
        if (SearchQuery.Parse(SearchText) is not null)
            _ = RefreshViewAsync();
    }

    private async Task DebouncedSearchAsync(CancellationToken ct)
    {
        try
        {
            await Task.Delay(200, ct);
            await RefreshViewAsync();
        }
        catch (OperationCanceledException)
        {
            // superseded by further typing or navigation
        }
    }

    private async Task RunSearchAsync(CancellationToken ct)
    {
        var queryText = SearchText;
        FileList.BeginSearch();

        SearchOutcome? outcome;
        // Search never surfaces hidden files/folders, regardless of the "Show hidden items"
        // browse setting — hidden entries are index noise (AppData, system junk) that bury the
        // results a search is actually for.
        if (SearchGlobal)
        {
            // Whole-PC: served straight from the MFT index, no live streaming.
            StatusText = $"Searching this PC for '{queryText}'…";
            outcome = await _searchService.SearchAllAsync(queryText, ct, includeHidden: false);
        }
        else
        {
            StatusText = $"Searching for '{queryText}'…";
            // Progress is constructed on the UI thread, so batches marshal back to it.
            var progress = new Progress<IReadOnlyList<SearchHit>>(batch =>
            {
                if (ct.IsCancellationRequested) return;
                FileList.AppendSearchHits(batch);
                StatusText = $"{FileList.Items.Count} result(s) so far for '{queryText}'…";
            });
            outcome = await _searchService.SearchAsync(CurrentPath, queryText, ct, progress, includeHidden: false);
        }

        if (outcome is null || ct.IsCancellationRequested) return;

        // Global hits come from MFT rows with no size/timestamp, so hydrate them from disk.
        await FileList.CompleteSearchAsync(outcome, queryText, hydrateMetadata: SearchGlobal, ct);
        if (ct.IsCancellationRequested) return;

        var scope = SearchGlobal ? "this PC" : CurrentPath;
        var suffix = outcome.Source switch
        {
            SearchResultSource.LiveScan => " — indexing in background…",
            SearchResultSource.StaleIndex => " — refreshing index…",
            _ when SearchGlobal && outcome.RefreshPending => " — indexing drives…",
            _ => " — indexed",
        };
        var truncated = outcome.Truncated ? " (showing first 1,000)" : "";
        StatusText = $"{outcome.Hits.Count} result(s) for '{queryText}' in {scope}{truncated}{suffix}";
    }

    [RelayCommand]
    private async Task ClearSearchAsync()
    {
        ClearSearchState();
        await RefreshViewAsync();
    }

    /// <summary>Resets the search box without triggering the debounced refresh.</summary>
    private void ClearSearchState()
    {
        _searchDebounceCts.Cancel();
        if (SearchText.Length > 0)
        {
            _suppressSearchRefresh = true;
            SearchText = "";
            _suppressSearchRefresh = false;
        }
    }

    public bool HasActiveSearch => SearchQuery.Parse(SearchText) is not null;

    // --- Index callbacks (fanned out to every tab by the shell) ---

    /// <summary>Set when a background index finished while this tab was hidden, so the folder
    /// sizes can be picked up the moment it comes to the front rather than for every tab at once.</summary>
    private bool _dirSizesStale;

    /// <summary>A volume's MFT index just finished: re-run an active whole-PC search, or — in
    /// normal browsing — refresh the folder sizes now that <c>dir_size_cache</c> is populated.</summary>
    internal void OnMftIndexRefreshed()
    {
        if (HasActiveSearch)
        {
            if (SearchGlobal) _ = RefreshViewAsync();
        }
        else if (IsActive)
        {
            _ = FileList.RefreshDirSizesAsync(CancellationToken.None);
        }
        else
        {
            _dirSizesStale = true;
        }
    }

    /// <summary>A background (re)crawl finished; re-run the search against the fresh index.</summary>
    internal void OnIndexRefreshed(string rootKey)
    {
        if (CurrentPath.Length == 0 || !HasActiveSearch) return;

        string currentKey;
        try
        {
            currentKey = PathKey.Canonicalize(CurrentPath);
        }
        catch (ArgumentException)
        {
            return;
        }
        if (!currentKey.Equals(rootKey, StringComparison.Ordinal) && !PathKey.IsUnder(currentKey, rootKey))
            return;

        _ = RefreshViewAsync();
    }

    /// <summary>Called by the pane when this tab becomes the visible one.</summary>
    public void OnActivated()
    {
        if (!_dirSizesStale) return;
        _dirSizesStale = false;
        if (!HasActiveSearch)
            _ = FileList.RefreshDirSizesAsync(CancellationToken.None);
    }

    // --- Item actions ---

    [RelayCommand]
    private void OpenItem(FileItemViewModel? item)
    {
        if (item is null) return;

        if (item.IsDirectory)
        {
            _ = NavigateToAsync(item.FullPath);
            return;
        }

        try
        {
            Process.Start(new ProcessStartInfo(item.FullPath) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            StatusText = $"Cannot open: {ex.Message}";
        }
    }

    /// <summary>Compute (or refresh) the recursive content size of the given directories.</summary>
    [RelayCommand]
    private async Task ComputeSizeAsync(IList<FileItemViewModel>? items)
    {
        if (items is null) return;
        var dirs = items.Where(i => i.IsDirectory).ToList();
        if (dirs.Count == 0) return;

        foreach (var dir in dirs)
            dir.IsSizeComputing = true;

        try
        {
            foreach (var dir in dirs)
            {
                try
                {
                    var result = await _sizeService.ComputeAsync(dir.FullPath, CancellationToken.None);
                    if (result is not null)
                    {
                        dir.SizeBytes = result.SizeBytes;
                        dir.SizeIncomplete = result.Incomplete;
                        dir.SizeComputedUtc = result.ComputedUtc;
                    }
                }
                finally
                {
                    dir.IsSizeComputing = false;
                }
            }
            StatusText = "Size scan complete";
        }
        finally
        {
            foreach (var dir in dirs)
                dir.IsSizeComputing = false;
        }
    }
}
