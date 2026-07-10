using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.App.ViewModels;

public sealed record BreadcrumbSegment(string Name, string FullPath);

public sealed partial class ShellViewModel : ObservableObject
{
    private readonly IDirectorySizeService _sizeService;
    private readonly ITagService _tagService;
    private readonly ISearchService _searchService;
    private readonly IFileTransferService _fileTransfer;
    private readonly IMftIndexService _mftIndex;
    private readonly AppSettings _settings;

    /// <summary>Reflects the current "Show hidden items" setting (may change while running).</summary>
    private bool IncludeHidden => _settings.ShowHiddenItems;

    private readonly Stack<string> _backStack = new();
    private readonly Stack<string> _forwardStack = new();
    private CancellationTokenSource _navigationCts = new();
    private CancellationTokenSource _searchDebounceCts = new();

    public FileListViewModel FileList { get; }
    public FolderTreeViewModel Tree { get; }
    public TagFilterViewModel TagFilter { get; }
    public BookmarksViewModel Bookmarks { get; }

    /// <summary>Raised after navigation so the view can select and scroll to a specific
    /// file (e.g. when a bookmarked file is opened).</summary>
    public event Action<string>? RevealFileRequested;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BreadcrumbSegments), nameof(CanGoUp))]
    private string _currentPath = "";

    [ObservableProperty]
    private string _statusText = "Ready";

    [ObservableProperty]
    private string _searchText = "";

    /// <summary>Search scope: true = whole PC (the MFT global index), false = the current
    /// folder subtree. Defaults to whole-PC, the point of the MFT index.</summary>
    [ObservableProperty]
    private bool _searchGlobal = true;

    /// <summary>MFT indexing state for the status bar ("Indexing C:…"); empty when idle.</summary>
    [ObservableProperty]
    private string _indexingStatus = "";

    partial void OnSearchGlobalChanged(bool value)
    {
        if (SearchQuery.Parse(SearchText) is not null)
            _ = RefreshViewAsync();
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

    public ShellViewModel(
        IFileSystemService fileSystem,
        ITagService tagService,
        IDirectorySizeService sizeService,
        ISearchService searchService,
        IFileTransferService fileTransfer,
        IBookmarkService bookmarkService,
        IMftIndexService mftIndex,
        DirSizeRepository dirSizeRepository,
        AppSettings settings)
    {
        _sizeService = sizeService;
        _tagService = tagService;
        _searchService = searchService;
        _fileTransfer = fileTransfer;
        _mftIndex = mftIndex;
        _settings = settings;

        FileList = new FileListViewModel(fileSystem, tagService, dirSizeRepository);
        FileList.PropertyChanged += OnFileListPropertyChanged;
        Tree = new FolderTreeViewModel(fileSystem);
        TagFilter = new TagFilterViewModel(tagService);
        Bookmarks = new BookmarksViewModel(bookmarkService);

        Tree.DirectorySelected += path => _ = NavigateToAsync(path);
        TagFilter.FilterChanged += () => _ = RefreshViewAsync();
        _searchService.IndexRefreshed += OnIndexRefreshed;
        _mftIndex.IndexRefreshed += OnMftIndexRefreshed;
        _mftIndex.StatusChanged += OnMftStatusChanged;
        IndexingStatus = _mftIndex.StatusText;
    }

    /// <summary>Overrides the initial directory (e.g. from the command line).</summary>
    public string? StartPath { get; set; }

    public async Task InitializeAsync()
    {
        await Bookmarks.LoadAsync();

        // Drives are enumerated off-thread; the roots must exist before the first reveal.
        await Tree.LoadDrivesAsync();

        var start = StartPath ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        await NavigateToAsync(start);

        // Portable devices can be slow to enumerate; append them after the first view loads.
        await Tree.LoadDevicesAsync();
    }

    // --- Bookmarks ---

    /// <summary>Opens a bookmark: navigate into a folder, or reveal a bookmarked file in its
    /// containing folder.</summary>
    public async Task OpenBookmarkAsync(BookmarkItemViewModel? bookmark)
    {
        if (bookmark is null) return;

        if (bookmark.IsDirectory)
        {
            if (!Directory.Exists(bookmark.FullPath))
            {
                StatusText = $"Folder not found: {bookmark.FullPath}";
                return;
            }
            await NavigateToAsync(bookmark.FullPath);
            return;
        }

        var parent = Path.GetDirectoryName(bookmark.FullPath);
        if (parent is null) return;
        await NavigateToAsync(parent);
        RevealFileRequested?.Invoke(bookmark.FullPath);
    }

    public async Task RemoveBookmarkAsync(BookmarkItemViewModel? bookmark)
    {
        if (bookmark is null) return;
        await Bookmarks.RemoveAsync(bookmark.FullPath);
    }

    /// <summary>Adds or removes bookmarks for the given entries. When any are not yet
    /// bookmarked, bookmarks them all; otherwise removes them all.</summary>
    public async Task ToggleBookmarksAsync(IReadOnlyList<(string FullPath, bool IsDirectory)> entries)
    {
        if (entries.Count == 0) return;

        var anyMissing = entries.Any(e => !Bookmarks.IsBookmarked(e.FullPath));
        foreach (var (fullPath, isDirectory) in entries)
        {
            if (anyMissing)
                await Bookmarks.AddAsync(fullPath, isDirectory);
            else
                await Bookmarks.RemoveAsync(fullPath);
        }
        StatusText = anyMissing
            ? $"Bookmarked {entries.Count} item(s)"
            : $"Removed {entries.Count} bookmark(s)";
    }

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

    private async Task SetPathAndLoadAsync(string path)
    {
        ClearSearchState(); // navigating exits search mode, like Explorer
        CurrentPath = path;
        ApplyDirectoryThumbnailScale(path); // restore this folder's tile/list preference
        BackCommand.NotifyCanExecuteChanged();
        ForwardCommand.NotifyCanExecuteChanged();
        UpCommand.NotifyCanExecuteChanged();
        await RefreshViewAsync();
    }

    private async Task RefreshViewAsync()
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
            else if (TagFilter.IsActive)
            {
                await FileList.LoadFlattenedAsync(
                    CurrentPath, TagFilter.CheckedTagIds,
                    TagFilter.MatchAll ? TagMatchMode.All : TagMatchMode.Any, IncludeHidden, ct);
                if (!ct.IsCancellationRequested)
                    StatusText = $"{FileList.Items.Count} tagged file(s) under {CurrentPath}";
            }
            else
            {
                await FileList.LoadDirectoryAsync(CurrentPath, IncludeHidden, ct);
                if (!ct.IsCancellationRequested)
                    StatusText = $"{FileList.Items.Count} item(s)";
            }

            await TagFilter.RefreshAsync(CurrentPath);
        }
        catch (OperationCanceledException)
        {
        }
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

    /// <summary>A volume's MFT index just finished: re-run an active whole-PC search, or — in
    /// normal browsing — refresh the folder sizes now that <c>dir_size_cache</c> is populated.</summary>
    private void OnMftIndexRefreshed(string rootKey)
    {
        Application.Current?.Dispatcher.InvokeAsync(() =>
        {
            if (SearchQuery.Parse(SearchText) is not null)
            {
                if (SearchGlobal) _ = RefreshViewAsync();
            }
            else
            {
                _ = FileList.RefreshDirSizesAsync(CancellationToken.None);
            }
        });
    }

    private void OnMftStatusChanged()
    {
        Application.Current?.Dispatcher.InvokeAsync(() => IndexingStatus = _mftIndex.StatusText);
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

    /// <summary>A background (re)crawl finished; re-run the search against the fresh index.</summary>
    private void OnIndexRefreshed(string rootKey)
    {
        if (CurrentPath.Length == 0 || SearchQuery.Parse(SearchText) is null) return;

        var currentKey = PathKey.Canonicalize(CurrentPath);
        if (!currentKey.Equals(rootKey, StringComparison.Ordinal) && !PathKey.IsUnder(currentKey, rootKey))
            return;

        Application.Current?.Dispatcher.InvokeAsync(() => _ = RefreshViewAsync());
    }

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

    // --- Clipboard (copy / cut / paste) ---

    [RelayCommand]
    private void CopySelection(IList<FileItemViewModel>? items) => SetClipboard(items, cut: false);

    [RelayCommand]
    private void CutSelection(IList<FileItemViewModel>? items) => SetClipboard(items, cut: true);

    private void SetClipboard(IList<FileItemViewModel>? items, bool cut)
    {
        var paths = items?.Where(i => !i.IsMissing).Select(i => i.FullPath).ToList();
        if (paths is not { Count: > 0 }) return;

        try
        {
            FileClipboard.SetFiles(paths, cut);
        }
        catch (System.Runtime.InteropServices.ExternalException ex)
        {
            StatusText = $"Clipboard error: {ex.Message}";
            return;
        }
        StatusText = $"{paths.Count} item(s) {(cut ? "cut" : "copied")}";
    }

    [RelayCommand]
    private async Task PasteAsync()
    {
        if (CurrentPath.Length == 0) return;

        (IReadOnlyList<string> Paths, bool IsCut)? clip;
        try
        {
            clip = FileClipboard.GetFiles();
        }
        catch (System.Runtime.InteropServices.ExternalException ex)
        {
            StatusText = $"Clipboard error: {ex.Message}";
            return;
        }
        if (clip is null) return;
        var (paths, isCut) = clip.Value;

        var destination = CurrentPath;
        StatusText = isCut ? "Moving…" : "Copying…";

        var errors = new List<string>();
        var moves = new List<(string From, string To)>();
        var pasted = 0;

        await Task.Run(() =>
        {
            foreach (var source in paths)
            {
                try
                {
                    if (isCut)
                    {
                        var dest = _fileTransfer.MoveInto(source, destination);
                        if (!dest.Equals(source, StringComparison.OrdinalIgnoreCase))
                        {
                            moves.Add((source, dest));
                            pasted++;
                        }
                    }
                    else
                    {
                        _fileTransfer.CopyInto(source, destination);
                        pasted++;
                    }
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                    or InvalidOperationException or FileNotFoundException or DirectoryNotFoundException)
                {
                    errors.Add(ex.Message);
                }
            }
        });

        // Tags follow moved entries (files exactly, directories with their whole subtree).
        foreach (var (from, to) in moves)
            await _tagService.MoveEntryAsync(from, to);

        if (isCut && pasted > 0)
        {
            try
            {
                FileClipboard.Clear(); // a cut is one-shot, like Explorer
            }
            catch (System.Runtime.InteropServices.ExternalException)
            {
            }
        }

        await RefreshViewAsync();
        var verb = isCut ? "Moved" : "Copied";
        StatusText = errors.Count > 0
            ? $"{verb} {pasted} item(s); {errors.Count} failed — {errors[0]}"
            : $"{verb} {pasted} item(s)";
    }

    /// <summary>Runs a user-defined command once per selected item it applies to.</summary>
    public void RunCustomCommand(CustomCommandDefinition command, IReadOnlyList<(string FullPath, bool IsDirectory)> targets)
    {
        var matched = targets
            .Where(t => t.IsDirectory ? command.AppliesToDirectories : command.AppliesToFiles)
            .ToList();

        foreach (var (fullPath, isDirectory) in matched)
        {
            try
            {
                Process.Start(new ProcessStartInfo(command.Command, CommandTemplate.Expand(command.Arguments, fullPath))
                {
                    UseShellExecute = true,
                    WorkingDirectory = isDirectory ? fullPath : Path.GetDirectoryName(fullPath) ?? "",
                });
            }
            catch (Exception ex)
            {
                StatusText = $"'{command.Name}' failed: {ex.Message}";
                return;
            }
        }

        if (matched.Count > 0)
            StatusText = $"Ran '{command.Name}' on {matched.Count} item(s)";
    }

    // --- Built-in "Open in…" launchers (files and directories) ---

    /// <summary>Opens a terminal rooted at the item's folder: the folder itself for a
    /// directory, or the containing folder for a file. Prefers Windows Terminal, falls
    /// back to PowerShell.</summary>
    public void OpenInTerminal(string fullPath, bool isDirectory)
    {
        var dir = isDirectory ? fullPath : Path.GetDirectoryName(fullPath);
        if (string.IsNullOrEmpty(dir)) return;

        try
        {
            Process.Start(new ProcessStartInfo("wt.exe", $"-d \"{dir}\"") { UseShellExecute = true });
        }
        catch
        {
            try
            {
                Process.Start(new ProcessStartInfo("powershell.exe")
                {
                    UseShellExecute = true,
                    WorkingDirectory = dir,
                });
            }
            catch (Exception ex)
            {
                StatusText = $"Cannot open terminal: {ex.Message}";
            }
        }
    }

    /// <summary>Opens the file or folder in VS Code. Uses the <c>code</c> launcher on PATH,
    /// then falls back to the standard user/system install locations of Code.exe.</summary>
    public void OpenInVSCode(string fullPath, bool isDirectory)
    {
        try
        {
            Process.Start(new ProcessStartInfo("code", $"\"{fullPath}\"") { UseShellExecute = true });
            return;
        }
        catch
        {
            // 'code' not on PATH — try the well-known install locations.
        }

        string[] candidates =
        [
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Programs", "Microsoft VS Code", "Code.exe"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                "Microsoft VS Code", "Code.exe"),
        ];
        foreach (var exe in candidates)
        {
            if (!File.Exists(exe)) continue;
            try
            {
                Process.Start(new ProcessStartInfo(exe, $"\"{fullPath}\"") { UseShellExecute = true });
                return;
            }
            catch (Exception ex)
            {
                StatusText = $"Cannot open VS Code: {ex.Message}";
                return;
            }
        }
        StatusText = "VS Code not found. Install it, or add 'code' to your PATH.";
    }

    [RelayCommand]
    private async Task RemoveMissingAsync(FileItemViewModel? item)
    {
        if (item is null || !item.IsMissing) return;
        await _tagService.RemoveFileAsync(item.FullPath);
        FileList.Items.Remove(item);
        await TagFilter.RefreshAsync(CurrentPath);
    }

    /// <summary>Called by views after tags were edited so chips/counts refresh.</summary>
    public async Task OnTagsChangedAsync()
    {
        await RefreshViewAsync();
    }
}
