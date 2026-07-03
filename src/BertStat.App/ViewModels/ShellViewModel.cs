using System.Diagnostics;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertStat.Core.Data;
using BertStat.Core.Services;

namespace BertStat.App.ViewModels;

public sealed record BreadcrumbSegment(string Name, string FullPath);

public sealed partial class ShellViewModel : ObservableObject
{
    private readonly IDirectorySizeService _sizeService;
    private readonly ITagService _tagService;

    private readonly Stack<string> _backStack = new();
    private readonly Stack<string> _forwardStack = new();
    private CancellationTokenSource _navigationCts = new();
    private CancellationTokenSource _scanCts = new();

    public FileListViewModel FileList { get; }
    public FolderTreeViewModel Tree { get; }
    public TagFilterViewModel TagFilter { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BreadcrumbSegments), nameof(CanGoUp))]
    private string _currentPath = "";

    [ObservableProperty]
    private string _statusText = "Ready";

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
        DirSizeRepository dirSizeRepository)
    {
        _sizeService = sizeService;
        _tagService = tagService;

        FileList = new FileListViewModel(fileSystem, tagService, dirSizeRepository);
        Tree = new FolderTreeViewModel(fileSystem);
        TagFilter = new TagFilterViewModel(tagService);

        Tree.DirectorySelected += path => _ = NavigateToAsync(path);
        TagFilter.FilterChanged += () => _ = RefreshViewAsync();
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
            if (TagFilter.IsActive)
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
    [RelayCommand]
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
