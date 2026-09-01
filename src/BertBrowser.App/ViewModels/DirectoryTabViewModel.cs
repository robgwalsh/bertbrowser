using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Search;

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
    private readonly IProcessLauncher _launcher;

    /// <summary>Only ever asked whether anything is indexed, so a whole-PC search that found
    /// nothing can say why rather than implying the PC is empty.</summary>
    private readonly BertBrowser.Core.Services.Mft.IMftIndexService _mftIndex;
    private readonly AppSettings _settings;

    /// <summary>Asked whether a path is browsable and whether it is inside a container. Never on
    /// the UI thread for anything that opens one — see <see cref="NavigateToAsync"/>.</summary>
    private readonly BertBrowser.Core.Services.Archives.IArchiveBrowser _archives;

    /// <summary>The cap on results from searching inside a container, matching the index's own.</summary>
    private const int ArchiveSearchLimit = 1000;

    private readonly Stack<string> _backStack = new();
    private readonly Stack<string> _forwardStack = new();
    private CancellationTokenSource _navigationCts = new();
    private CancellationTokenSource _searchDebounceCts = new();

    public FileListViewModel FileList { get; }

    /// <summary>This tab's preview pane. Owned here, and constructed here rather than injected,
    /// for the reason <see cref="FileList"/> is: it is part of what a tab <em>is</em>, and there
    /// is one per tab because the selection it follows is per tab.</summary>
    public PreviewPaneViewModel Preview { get; }

    /// <summary>Whether this tab shows its preview pane. Per tab, so one pane can be previewing
    /// while another shows a full-width list; the persisted setting is what a new tab starts
    /// from, and toggling writes it back so the next tab inherits the choice.</summary>
    [ObservableProperty]
    private bool _isPreviewVisible;

    partial void OnIsPreviewVisibleChanged(bool value)
    {
        _settings.ShowPreviewPane = value;
        if (value) Preview.Show(SelectedItems);
    }

    [RelayCommand]
    private void TogglePreview() => IsPreviewVisible = !IsPreviewVisible;

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

    /// <summary>This tab's own search box, which is <em>always</em> scoped to the current folder's
    /// subtree. Whole-PC search is a separate field in the window header, so neither box has a mode
    /// the user has to notice before typing.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ActiveSearchText), nameof(IsGlobalSearch))]
    private string _searchText = "";

    /// <summary>The header's whole-PC query (the MFT global index). Held per tab, not per window,
    /// because the hits land in <em>this</em> tab's file list — the header field just binds through
    /// the active tab. Exclusive with <see cref="SearchText"/>: filling one empties the other.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ActiveSearchText), nameof(IsGlobalSearch))]
    private string _globalSearchText = "";

    /// <summary>The query actually driving the listing: the whole-PC one when it has text,
    /// otherwise the tab's own.</summary>
    public string ActiveSearchText => GlobalSearchText.Length > 0 ? GlobalSearchText : SearchText;

    /// <summary>True when the listing is being driven by the header's whole-PC field rather than
    /// by this tab's folder-local box.</summary>
    public bool IsGlobalSearch => GlobalSearchText.Length > 0;

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

    /// <summary>
    /// An item to highlight, then forgotten. Set by <c>ShellViewModel.OpenRequestAsync</c> for
    /// Explorer's <c>/select</c>, and consumed by <c>DirectoryTabView</c> — the selection lives in
    /// the <c>ListView</c>, so the view is the only thing that can apply it, and only once the rows
    /// it names actually exist.
    /// </summary>
    /// <remarks>
    /// Observable on purpose. The row may already be on screen — asking to select something in the
    /// folder already open is the common case — in which case no listing reload is coming and
    /// waiting for one would mean waiting forever. So the view applies this on <em>either</em>
    /// signal: this property changing, or the listing being replaced.
    /// </remarks>
    [ObservableProperty]
    private string? _pendingSelection;

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
        AppSettings settings,
        IProcessLauncher launcher,
        BertBrowser.Core.Services.Mft.IMftIndexService mftIndex,
        BertBrowser.Core.Services.Archives.IArchiveBrowser archives,
        BertBrowser.Core.Services.Archives.IArchiveReader archiveReader,
        BertBrowser.Core.Services.Archives.IArchivePasswords archivePasswords)
    {
        _searchService = searchService;
        _settings = settings;
        _launcher = launcher;
        _mftIndex = mftIndex;
        _archives = archives;

        FileList = new FileListViewModel(fileSystem, dirSizeRepository, settings, archives);
        FileList.PropertyChanged += OnFileListPropertyChanged;
        Preview = new PreviewPaneViewModel(settings, archives, archiveReader, archivePasswords);
        _isPreviewVisible = settings.ShowPreviewPane;
        _refreshTimer.Tick += OnRefreshTick;
    }

    /// <summary>Cancels anything in flight and unsubscribes. A tab is closable, unlike the shell,
    /// so its subscriptions have to be given back.</summary>
    /// <summary>Raised as the tab is torn down, so anything holding it — a folder comparison — can
    /// let go. Distinct from <see cref="LocationChanged"/>: a tab that closed did not navigate.</summary>
    public event Action<DirectoryTabViewModel>? Closing;

    public void Dispose()
    {
        Closing?.Invoke(this);
        _navigationCts.Cancel();
        _searchDebounceCts.Cancel();
        _searchStopCts?.Cancel();
        _searchStopCts?.Dispose();
        FileList.PropertyChanged -= OnFileListPropertyChanged;
        Preview.Dispose();
        StopWatching();
    }

    // --- Live refresh ---

    /// <summary>
    /// Watches the folder this tab is showing, so a file created, deleted or renamed by anything
    /// else appears without an F5.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Separate from <see cref="IndexWatcherService"/>, which watches indexed <em>roots</em> to keep
    /// the search index fresh. This one is per open folder, not recursive, and exists purely so the
    /// visible list is true.
    /// </para>
    /// <para>
    /// The refresh goes through <see cref="FileListViewModel.MergeDirectoryAsync"/>, never a load:
    /// a load replaces the collection, and the view focuses the list when that happens — which
    /// would drop the selection and steal the caret out of another pane every time a file landed
    /// on disk.
    /// </para>
    /// </remarks>
    private FileSystemWatcher? _watcher;

    /// <summary>Coalesces a burst of events into one refresh; a single save can raise several.</summary>
    private readonly System.Windows.Threading.DispatcherTimer _refreshTimer = new()
    {
        Interval = TimeSpan.FromMilliseconds(250),
    };

    private void StartWatching(string path)
    {
        StopWatching();
        if (path.Length == 0) return;

        // Nothing inside an archive can be watched, and this is an explicit skip rather than a
        // reliance on the catch below: FileSystemWatcher throws ArgumentException on a path that is
        // not a directory, which the catch would swallow into "no live refresh" — working, but by
        // accident, and invisibly. The container itself is deliberately not watched either: a file
        // being rewritten under a browsing session has no good answer, and F5 genuinely re-reads
        // because the cache key carries its length and write time.
        if (ArchivePath.LooksVirtual(path) && !Directory.Exists(path)) return;

        try
        {
            _watcher = new FileSystemWatcher(path)
            {
                // Everything a row shows. Not IncludeSubdirectories: this list is one folder deep,
                // and watching a tree would raise an event for every file under it.
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName |
                               NotifyFilters.Size | NotifyFilters.LastWrite | NotifyFilters.Attributes,
                EnableRaisingEvents = true,
            };

            _watcher.Created += OnFolderChanged;
            _watcher.Deleted += OnFolderChanged;
            _watcher.Renamed += OnFolderChanged;
            _watcher.Changed += OnFolderChanged;

            // An overflow means events were missed, so the coalesced merge is exactly the right
            // answer — it compares against disk rather than trying to apply what it was told.
            _watcher.Error += OnFolderChanged;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            // A folder that cannot be watched — a network share, a device that went away — is not
            // an error worth reporting. It simply behaves as it did before: F5 refreshes it.
            _watcher = null;
        }
    }

    private void StopWatching()
    {
        _refreshTimer.Stop();
        if (_watcher is null) return;

        _watcher.EnableRaisingEvents = false;
        _watcher.Created -= OnFolderChanged;
        _watcher.Deleted -= OnFolderChanged;
        _watcher.Renamed -= OnFolderChanged;
        _watcher.Changed -= OnFolderChanged;
        _watcher.Error -= OnFolderChanged;
        _watcher.Dispose();
        _watcher = null;
    }

    /// <summary>Watcher events arrive on a thread-pool thread; the timer restarts on the UI one.</summary>
    private void OnFolderChanged(object sender, EventArgs e) =>
        System.Windows.Application.Current?.Dispatcher.InvokeAsync(() =>
        {
            _refreshTimer.Stop();
            _refreshTimer.Start();
        });

    private async void OnRefreshTick(object? sender, EventArgs e)
    {
        _refreshTimer.Stop();

        // A search result is not a folder listing, and a transfer's own fan-out through
        // RefreshTabsShowingAsync already owns the refresh while one is running.
        if (FileList.IsFlattened || CurrentPath.Length == 0) return;

        try
        {
            await FileList.MergeDirectoryAsync(CurrentPath, IncludeHidden, _navigationCts.Token);

            // Re-checked after the await, not only before it. A search can begin while the merge is
            // in flight — the merge itself refuses a flattened list, but the status line was written
            // regardless, replacing "3 result(s) for 'report'" with a count of the folder behind it.
            if (!FileList.IsFlattened)
                StatusText = $"{FileList.Items.Count} item(s)";
        }
        catch (OperationCanceledException)
        {
        }
    }

    // --- Navigation ---

    [RelayCommand]
    public async Task NavigateToAsync(string path)
    {
        if (path.Equals(CurrentPath, StringComparison.OrdinalIgnoreCase)) return;

        // Two stages, and the second one is deliberately not an answer.
        //
        // A real directory is the common case and settles immediately. Otherwise LooksVirtual — a
        // pure segment scan, no disk — says whether any part of the path names an archive. If it
        // does we go ahead and let the *load* find out, because deciding here would mean opening a
        // container on the UI thread, of a file that may be on a dead network share. That is the
        // call the preview pane already refuses to make on this thread, for the same reason.
        //
        // So a damaged, encrypted or absent archive becomes a banner in the list rather than a
        // refusal at the gate, which is also the only way the banner can offer Unlock.
        if (!Directory.Exists(path))
        {
            if (!ArchivePath.LooksVirtual(path))
            {
                StatusText = $"Folder not found: {path}";
                return;
            }
        }
        else
        {
            path = ResolveInaccessibleJunction(path);
        }
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
        StartWatching(path); // so anything else changing this folder shows up without an F5
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
            if (HasActiveSearch)
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
        if (value.Length > 0) Suppressed(() => GlobalSearchText = "");
        QueueSearch();
    }

    partial void OnGlobalSearchTextChanged(string value)
    {
        _searchDebounceCts.Cancel();
        if (_suppressSearchRefresh) return;
        if (value.Length > 0) Suppressed(() => SearchText = "");
        QueueSearch();
    }

    /// <summary>Runs a change to the search boxes without its handler queueing a search of its own —
    /// used to empty the box the user isn't typing in, since only one query is ever live.</summary>
    private void Suppressed(Action change)
    {
        _suppressSearchRefresh = true;
        change();
        _suppressSearchRefresh = false;
    }

    /// <summary>Restarts the 200 ms debounce. Shared by both boxes, since only one of them holds
    /// the live query.</summary>
    private void QueueSearch()
    {
        _searchDebounceCts = new CancellationTokenSource();
        _ = DebouncedSearchAsync(_searchDebounceCts.Token);
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
        try
        {
            await RunSearchCoreAsync(ct);
        }
        catch (OperationCanceledException) when (!ct.IsCancellationRequested)
        {
            // A *stop* rather than a supersede: the navigation token is untouched, so nothing else
            // is coming to end this search. BeginSearch raised IsLoading and only
            // CompleteSearchAsync lowers it, so without this the bar spins for ever and
            // UiSession.Settle — which waits on exactly that flag — never returns.
            IsSearchRunning = false;
            FileList.EndSearch();
            StatusText =
                $"Stopped — {FileList.Items.Count} result(s) so far for '{ActiveSearchText}'";
        }
    }

    private async Task RunSearchCoreAsync(CancellationToken ct)
    {
        var queryText = ActiveSearchText;
        var global = IsGlobalSearch;

        // A query that cannot be used is reported *before* the list is cleared, so what is on
        // screen stays there under the banner. Emptying the list first would make a query
        // half-way through being typed look like one that found nothing.
        if (ParsedSearch.Problem is { } problem)
        {
            FileList.ErrorMessage = problem;
            StatusText = problem;
            return;
        }

        var needsContent = ParsedSearch.Query!.NeedsContent;
        FileList.BeginSearch();
        FileList.ShowsContentMatches = needsContent;

        // Escape means "stop this, keep what you found" while a search is running, so the reading
        // pass gets a token of its own rather than being cancelled only by navigation. Both are
        // linked: typing more still supersedes the run, and there the outcome is discarded below.
        _searchStopCts?.Dispose();
        _searchStopCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        var stopToken = _searchStopCts.Token;
        IsSearchRunning = needsContent;

        // How much of the reading pass is left. A separate channel from the hits, because it
        // answers a different question — and during the tens of seconds a whole-tree grep can take
        // it is the only thing worth saying.
        var contentProgress = new Progress<ContentScanProgress>(p =>
        {
            if (ct.IsCancellationRequested) return;
            StatusText = p.FilesToRead > 0
                ? $"Reading {p.FilesRead:N0} of {p.FilesToRead:N0} file(s) for '{queryText}' — " +
                  $"{FileList.Items.Count} result(s) so far — Esc to stop"
                : $"Searching for '{queryText}'…";
        });

        SearchOutcome? outcome;
        // Search never surfaces hidden files/folders, regardless of the "Show hidden items"
        // browse setting — hidden entries are index noise (AppData, system junk) that bury the
        // results a search is actually for.
        if (global)
        {
            // Whole-PC: the names come straight from the MFT index. A content term then turns that
            // instant answer into tens of seconds of reading, which is why this streams now.
            StatusText = $"Searching this PC for '{queryText}'…";
            var globalProgress = new Progress<IReadOnlyList<SearchHit>>(batch =>
            {
                if (ct.IsCancellationRequested) return;
                FileList.AppendSearchHits(batch);
            });
            outcome = await _searchService.SearchAllAsync(
                queryText, stopToken, includeHidden: false,
                liveBatches: needsContent ? globalProgress : null,
                contentProgress: needsContent ? contentProgress : null);
        }
        else if (_archives.Resolve(CurrentPath) is { } here)
        {
            // Refused rather than answered wrongly, and this branch needs its own check because it
            // never reaches SearchService: an archive entry has no file on disk, so its candidate
            // carries no content — and an unread candidate counts as a possible match by design.
            // Run the walk anyway and every entry in the container comes back as a hit.
            if (needsContent)
            {
                const string message = "content: can't look inside archives — extract it first.";
                FileList.ErrorMessage = message;
                StatusText = message;
                FileList.EndSearch();
                IsSearchRunning = false;
                return;
            }

            // Inside a container the index is no help — it holds nothing about archive contents,
            // and handing it a virtual root would crawl a path that does not exist straight into
            // fs_entry. It is also unnecessary: the listing already read the whole directory, so
            // this is a walk over something in memory and is instant.
            StatusText = $"Searching {Path.GetFileName(here.ArchiveFile)} for '{queryText}'…";

            var hits = await Task.Run(() => ArchiveSearchScanner.Search(
                _archives.ReadArchive(here.ArchiveFile),
                here.ArchiveFile, here.EntryPath, ParsedSearch.Query!, ArchiveSearchLimit, ct), ct);

            if (ct.IsCancellationRequested) return;
            FileList.AppendSearchHits(hits);
            outcome = new SearchOutcome(
                hits, Truncated: hits.Count >= ArchiveSearchLimit, SearchResultSource.LiveScan,
                RefreshPending: false);
        }
        else
        {
            StatusText = $"Searching for '{queryText}'…";
            // Progress is constructed on the UI thread, so batches marshal back to it.
            var progress = new Progress<IReadOnlyList<SearchHit>>(batch =>
            {
                if (ct.IsCancellationRequested) return;
                FileList.AppendSearchHits(batch);

                // The content pass owns the status line once it starts reporting, or the two
                // channels would fight over one row of text every hundred milliseconds.
                if (!needsContent)
                    StatusText = $"{FileList.Items.Count} result(s) so far for '{queryText}'…";
            });
            outcome = await _searchService.SearchAsync(
                CurrentPath, queryText, stopToken, progress, includeHidden: false,
                contentProgress: needsContent ? contentProgress : null);
        }

        IsSearchRunning = false;

        // The navigation token, not the stop token: typing more supersedes this run and its
        // results belong to a query that is no longer on screen. A *stop* leaves that token alone,
        // so the floor the pass returned is kept and shown.
        if (outcome is null || ct.IsCancellationRequested) return;

        // Global hits come from MFT rows with no size/timestamp, so hydrate them from disk.
        await FileList.CompleteSearchAsync(outcome, queryText, hydrateMetadata: global, ct);
        if (ct.IsCancellationRequested) return;

        var scope = global ? "this PC" : CurrentPath;

        // With no index at all, "indexing in background…" would be a promise nothing is keeping:
        // the drives are not being read, because the helper that reads them was declined or could
        // not run. Say so instead — the status bar carries the retry.
        var noIndex = global && !_mftIndex.AnyIndexed && !_mftIndex.IsBuilding;

        var suffix = outcome.Source switch
        {
            // What the reading pass did comes first, because it explains the count more directly
            // than anything about the index does: a content search that stopped at a ceiling found
            // what it found in the part of the disk it reached, and saying "0 results" instead
            // would report an absence the search never established.
            _ when outcome.Cancelled => " — stopped",
            _ when outcome.ContentScan is { } scan && scan.Limit != ContentScanLimit.None =>
                ContentLimitSuffix(scan),
            _ when outcome.ContentScan is { Unreadable: > 0 } some =>
                $" — {some.Unreadable:N0} file(s) could not be read",
            _ when outcome.ContentScan is { Truncated: > 0 } big =>
                $" — {big.Truncated:N0} file(s) were only searched to their first " +
                $"{ByteSizeFormatter.Format(ContentSearchRules.MaxBytesPerFile)}",
            // A size or date filter against an index that holds no lengths cannot match anything,
            // so "0 results" would report an empty disk rather than an unmeasured one. Said first
            // because it explains the count, which every other suffix assumes is meaningful.
            _ when outcome.ScopeLacksMetadata =>
                " — this drive has no size or date data, so those filters can't match",
            _ when noIndex => " — the search index is off",
            SearchResultSource.LiveScan => " — indexing in background…",
            SearchResultSource.StaleIndex => " — refreshing index…",
            _ when global && outcome.RefreshPending => " — indexing drives…",
            _ => " — indexed",
        };
        var truncated = outcome.Truncated ? " (showing first 1,000)" : "";
        StatusText = $"{outcome.Hits.Count} result(s) for '{queryText}' in {scope}{truncated}{suffix}";
    }

    /// <summary>
    /// Where a content search ran out of budget, named rather than merely counted.
    /// </summary>
    /// <remarks>
    /// A whole-PC content search walks the index in path order with no <c>ORDER BY</c>, so running
    /// out means stopping somewhere alphabetical — quite possibly before <c>C:\Users</c>. "Searched
    /// the first 50,000 files" is true and still reads as "your PC has no such file"; naming where
    /// it reached is what turns that into a bound the reader can act on.
    /// </remarks>
    private static string ContentLimitSuffix(ContentScanReport scan)
    {
        var where = scan.LastPathExamined is { Length: > 0 } path ? $", up to {path}" : "";
        return scan.Limit switch
        {
            ContentScanLimit.Bytes =>
                $" — stopped after reading {ByteSizeFormatter.Format(scan.BytesRead)}{where}",
            _ => $" — searched the first {scan.FilesRead:N0} file(s){where}",
        };
    }

    /// <summary>True while a reading pass is in flight, so Escape can stop it instead of clearing.</summary>
    [ObservableProperty]
    private bool _isSearchRunning;

    private CancellationTokenSource? _searchStopCts;

    /// <summary>
    /// Stops a running content search but keeps what it found.
    /// </summary>
    /// <remarks>
    /// Cancels a token linked to — but distinct from — the navigation one, so the pass returns its
    /// floor and the outcome is still used. Cancelling navigation instead would discard the
    /// results, which is right when the user typed another character and wrong when they asked it
    /// to stop.
    /// </remarks>
    [RelayCommand]
    private void StopSearch()
    {
        if (!IsSearchRunning) return;
        _searchStopCts?.Cancel();
        IsSearchRunning = false;
    }

    [RelayCommand]
    private async Task ClearSearchAsync()
    {
        ClearSearchState();
        await RefreshViewAsync();
    }

    /// <summary>Resets both search boxes without triggering the debounced refresh.</summary>
    private void ClearSearchState()
    {
        _searchDebounceCts.Cancel();
        if (SearchText.Length == 0 && GlobalSearchText.Length == 0) return;
        Suppressed(() =>
        {
            SearchText = "";
            GlobalSearchText = "";
        });
    }

    private (string Text, SearchQueryParse Parse) _parsedSearch = ("", default);

    /// <summary>
    /// The parsed search box, memoised on its text. Parsing is cheap but no longer trivial — it
    /// lexes, builds a node tree and may compile a regular expression — and this is read several
    /// times per keystroke and again on every index-refresh callback, so re-parsing per read
    /// would do that work for nothing.
    /// </summary>
    private SearchQueryParse ParsedSearch
    {
        get
        {
            var text = ActiveSearchText;
            if (!string.Equals(_parsedSearch.Text, text, StringComparison.Ordinal))
                _parsedSearch = (text, SearchQuery.Parse(text));
            return _parsedSearch.Parse;
        }
    }

    /// <summary>
    /// Whether the box holds something the user means as a search. Deliberately true for a query
    /// that <em>cannot</em> be used as well as one that can: a half-typed <c>size:&gt;</c> must
    /// leave the view in search mode showing the reason, not flip back to the directory listing
    /// for one keystroke and then flip forward again.
    /// </summary>
    public bool HasActiveSearch => ParsedSearch.Query is not null || ParsedSearch.Problem is not null;

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
            if (IsGlobalSearch) _ = RefreshViewAsync();
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
    private void OpenItem(FileItemViewModel? item) => Open(item, elevated: false);

    /// <summary>
    /// Opens <paramref name="item"/> — navigating if it is a folder, otherwise handing it to the
    /// shell to launch. <paramref name="elevated"/> asks for administrator rights, which is a real
    /// UAC prompt; see <see cref="IProcessLauncher"/> for why it is one rather than a silent
    /// inheritance of ours.
    /// </summary>
    public void Open(FileItemViewModel? item, bool elevated)
    {
        if (item is null) return;

        if (item.IsDirectory)
        {
            // "Run this folder as administrator" means nothing; navigate as usual.
            _ = NavigateToAsync(item.FullPath);
            return;
        }

        // The elevated arm comes before the archive one, and the order is load-bearing: put it
        // after and Ctrl+Shift+double-click on a .zip silently stops meaning "run as administrator".
        if (!elevated &&
            _settings.EnterArchivesOnDoubleClick &&
            ArchiveFormats.IsArchiveName(item.Name) &&
            !FileList.IsInsideArchive)
        {
            _ = EnterArchiveOrLaunchAsync(item);
            return;
        }

        // Nothing inside a container has a path another program can open, and ProcessLauncher would
        // report that as a launch failure. Say the useful thing instead.
        if (FileList.IsInsideArchive)
        {
            StatusText = $"Extract {item.Name} to open it.";
            return;
        }

        if (_launcher.Launch(item.FullPath, elevated: elevated) is { } message)
            StatusText = message;
    }

    /// <summary>
    /// Double-clicking something whose name says archive: walk into it, or hand it to the program
    /// that owns it.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>A name is a claim, and only the bytes settle it.</b> Plenty of files are called
    /// <c>.zip</c> without being one, and taking the user into an error page instead of opening
    /// their file would be a regression they never asked for. So a container whose bytes are not
    /// what the suffix claims — <see cref="ArchiveFailure.Damaged"/> — falls back to launching,
    /// exactly as it did before archives were browsable.
    /// </para>
    /// <para>
    /// Every other failure <em>does</em> navigate, and that is the point of separating them: an
    /// encrypted archive is a real archive, and the banner it lands on is what offers to unlock it.
    /// Launching that one instead would hide the only way in.
    /// </para>
    /// <para>
    /// The read is off the UI thread and cached, so the navigation that follows re-uses it rather
    /// than opening the file twice.
    /// </para>
    /// </remarks>
    private async Task EnterArchiveOrLaunchAsync(FileItemViewModel item)
    {
        var path = item.FullPath;

        // The list is genuinely loading while this runs — opening a large 7z takes a moment — so
        // say so rather than leaving the pane looking idle. It is also the flag quiescence is
        // measured by, which is what makes the step assertable in a script instead of a race.
        FileList.IsLoading = true;
        ArchiveIndex index;
        try
        {
            index = await Task.Run(() => _archives.ReadArchive(path));
        }
        catch (Exception)
        {
            FileList.IsLoading = false;
            throw;
        }

        if (index.Failure == ArchiveFailure.Damaged)
        {
            // Not what its name claims. Hand it back to whatever owns the extension, exactly as
            // this did before archives were browsable.
            FileList.IsLoading = false;
            if (_launcher.Launch(path, elevated: false) is { } message) StatusText = message;
            return;
        }

        // Left set on purpose: the load this starts owns the flag from here and clears it in its
        // own finally, so the pane never blinks out of its loading state and back into it.
        await NavigateToAsync(path);
    }
}
