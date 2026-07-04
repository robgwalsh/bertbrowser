using System.Diagnostics;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed record BreadcrumbSegment(string Name, string FullPath);

public sealed partial class ShellViewModel : ObservableObject
{
    private readonly IDirectorySizeService _sizeService;
    private readonly ITagService _tagService;
    private readonly ISearchService _searchService;

    private readonly Stack<string> _backStack = new();
    private readonly Stack<string> _forwardStack = new();
    private CancellationTokenSource _navigationCts = new();
    private CancellationTokenSource _scanCts = new();
    private CancellationTokenSource _searchDebounceCts = new();

    public FileListViewModel FileList { get; }
    public FolderTreeViewModel Tree { get; }
    public TagFilterViewModel TagFilter { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BreadcrumbSegments), nameof(CanGoUp))]
    private string _currentPath = "";

    [ObservableProperty]
    private string _statusText = "Ready";

    [ObservableProperty]
    private string _searchText = "";

    private int _activeScans;

    [ObservableProperty]
    private bool _isScanning;

    partial void OnIsScanningChanged(bool value) => CancelScansCommand.NotifyCanExecuteChanged();

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
        DirSizeRepository dirSizeRepository)
    {
        _sizeService = sizeService;
        _tagService = tagService;
        _searchService = searchService;

        FileList = new FileListViewModel(fileSystem, tagService, dirSizeRepository);
        Tree = new FolderTreeViewModel(fileSystem);
        TagFilter = new TagFilterViewModel(tagService);

        Tree.DirectorySelected += path => _ = NavigateToAsync(path);
        TagFilter.FilterChanged += () => _ = RefreshViewAsync();
        _searchService.IndexRefreshed += OnIndexRefreshed;
    }

    /// <summary>Overrides the initial directory (e.g. from the command line).</summary>
    public string? StartPath { get; set; }

    public async Task InitializeAsync()
    {
        var start = StartPath ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        await NavigateToAsync(start);
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

        if (CurrentPath.Length > 0)
            _backStack.Push(CurrentPath);
        _forwardStack.Clear();

        await SetPathAndLoadAsync(path);
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
                    TagFilter.MatchAll ? TagMatchMode.All : TagMatchMode.Any, ct);
                if (!ct.IsCancellationRequested)
                    StatusText = $"{FileList.Items.Count} tagged file(s) under {CurrentPath}";
            }
            else
            {
                await FileList.LoadDirectoryAsync(CurrentPath, ct);
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
        StatusText = $"Searching for '{queryText}'…";

        // Progress is constructed on the UI thread, so batches marshal back to it.
        var progress = new Progress<IReadOnlyList<SearchHit>>(batch =>
        {
            if (ct.IsCancellationRequested) return;
            FileList.AppendSearchHits(batch);
            StatusText = $"{FileList.Items.Count} result(s) so far for '{queryText}'…";
        });

        var outcome = await _searchService.SearchAsync(CurrentPath, queryText, ct, progress);
        if (outcome is null || ct.IsCancellationRequested) return;

        await FileList.CompleteSearchAsync(outcome, queryText, ct);
        if (ct.IsCancellationRequested) return;

        var suffix = outcome.Source switch
        {
            SearchResultSource.LiveScan => " — indexing in background…",
            SearchResultSource.StaleIndex => " — refreshing index…",
            _ => " — indexed",
        };
        var truncated = outcome.Truncated ? " (showing first 1,000)" : "";
        StatusText = $"{outcome.Hits.Count} result(s) for '{queryText}' under {CurrentPath}{truncated}{suffix}";
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

        var progress = new Progress<DirScanProgress>(p =>
            StatusText = $"Scanning… {p.DirectoriesScanned} folders, {ByteSizeFormatter.Format(p.BytesSoFar)}");

        foreach (var dir in dirs)
            dir.IsSizeComputing = true;

        _activeScans++;
        IsScanning = true;
        try
        {
            foreach (var dir in dirs)
            {
                var result = await _sizeService.ComputeAsync(dir.FullPath, _scanCts.Token, progress);
                if (result is not null)
                {
                    dir.SizeBytes = result.SizeBytes;
                    dir.SizeIncomplete = result.Incomplete;
                    dir.SizeComputedUtc = result.ComputedUtc;
                }
                dir.IsSizeComputing = false;
            }
            StatusText = "Size scan complete";
        }
        finally
        {
            foreach (var dir in dirs)
                dir.IsSizeComputing = false;
            if (--_activeScans == 0)
                IsScanning = false;
        }
    }

    /// <summary>Toolbar action: compute sizes for every direct child folder of the current directory.</summary>
    [RelayCommand]
    private async Task ComputeSizesHereAsync()
    {
        var dirs = FileList.Items.Where(i => i.IsDirectory).ToList();
        if (dirs.Count > 0)
            await ComputeSizeAsync(dirs);
    }

    /// <summary>Cancels in-flight size scans; the cache keeps its previous values.</summary>
    [RelayCommand(CanExecute = nameof(IsScanning))]
    private void CancelScans()
    {
        _scanCts.Cancel();
        _scanCts = new CancellationTokenSource();
        StatusText = "Size scan cancelled";
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
