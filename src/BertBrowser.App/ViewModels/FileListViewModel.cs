using System.Collections.ObjectModel;
using BertBrowser.App.Interop;
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
    Tags,
    RelativePath,
}

public sealed partial class FileListViewModel : ObservableObject
{
    private readonly IFileSystemService _fileSystem;
    private readonly ITagService _tagService;
    private readonly DirSizeRepository _dirSizeRepository;

    [ObservableProperty]
    private bool _isLoading;

    [ObservableProperty]
    private bool _isFlattened;

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    private SortColumn _sortBy = SortColumn.Name;

    [ObservableProperty]
    private bool _sortDescending;

    [ObservableProperty]
    private ObservableCollection<FileItemViewModel> _items = new();

    public FileListViewModel(IFileSystemService fileSystem, ITagService tagService, DirSizeRepository dirSizeRepository)
    {
        _fileSystem = fileSystem;
        _tagService = tagService;
        _dirSizeRepository = dirSizeRepository;
    }

    /// <summary>Normal browsing: direct children of <paramref name="path"/>.</summary>
    public async Task LoadDirectoryAsync(string path, CancellationToken ct)
    {
        IsLoading = true;
        IsFlattened = false;
        ErrorMessage = null;
        try
        {
            var entries = await Task.Run(() => _fileSystem.ListDirectory(path), ct);
            ct.ThrowIfCancellationRequested();

            var items = await Task.Run(() =>
            {
                var vms = entries.Select(e => new FileItemViewModel(e)).ToList();
                SortInPlace(vms);
                return vms;
            }, ct);

            ReplaceItems(items);

            await HydrateDirSizesAsync(items, ct);
            await HydrateTagsAsync(items.Where(i => !i.IsDirectory).ToList(), ct);
        }
        catch (OperationCanceledException)
        {
            // superseded by a newer navigation
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
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
    /// Flattened tag-filter mode: all tagged files recursively under <paramref name="root"/>
    /// matching the filter. Entirely DB-driven.
    /// </summary>
    public async Task LoadFlattenedAsync(
        string root, IReadOnlyCollection<long> tagIds, TagMatchMode mode, CancellationToken ct)
    {
        IsLoading = true;
        IsFlattened = true;
        ErrorMessage = null;
        try
        {
            var tagged = await _tagService.QueryTaggedFilesUnderAsync(root, tagIds, mode);
            ct.ThrowIfCancellationRequested();

            var allTags = (await _tagService.GetAllTagsAsync()).ToDictionary(t => t.Name, StringComparer.OrdinalIgnoreCase);
            var rootDisplay = PathKey.NormalizeDisplay(root);

            var items = await Task.Run(() =>
            {
                var vms = new List<FileItemViewModel>(tagged.Count);
                foreach (var file in tagged)
                {
                    var relative = Path.GetRelativePath(rootDisplay, Path.GetDirectoryName(file.DisplayPath) ?? rootDisplay);
                    if (relative == ".") relative = "";
                    var tags = file.Tags
                        .Select(n => allTags.TryGetValue(n, out var t) ? t : new Tag(-1, n, null))
                        .ToList();
                    var vm = new FileItemViewModel(file.DisplayPath, relative, tags);
                    vm.HydrateFromDisk();
                    vms.Add(vm);
                }
                SortInPlace(vms);
                return vms;
            }, ct);

            ReplaceItems(items);
        }
        catch (OperationCanceledException)
        {
        }
        finally
        {
            IsLoading = false;
        }
    }

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

    private async Task HydrateTagsAsync(IReadOnlyList<FileItemViewModel> files, CancellationToken ct)
    {
        if (files.Count == 0) return;

        var map = await _tagService.GetTagsForPathsAsync(files.Select(f => f.FullPath).ToList());
        ct.ThrowIfCancellationRequested();

        foreach (var file in files)
        {
            if (map.TryGetValue(PathKey.Canonicalize(file.FullPath), out var tags))
                file.Tags = tags;
        }
    }

    private void ReplaceItems(IReadOnlyList<FileItemViewModel> items) =>
        Items = new ObservableCollection<FileItemViewModel>(items);

    private void SortInPlace(List<FileItemViewModel> items)
    {
        Comparison<FileItemViewModel> cmp = SortBy switch
        {
            SortColumn.Size => (a, b) => a.SizeSortKey.CompareTo(b.SizeSortKey),
            SortColumn.Type => (a, b) => string.Compare(a.TypeName, b.TypeName, StringComparison.OrdinalIgnoreCase),
            SortColumn.Modified => (a, b) => a.ModifiedUtc.CompareTo(b.ModifiedUtc),
            SortColumn.Tags => (a, b) => string.Compare(a.TagsSortKey, b.TagsSortKey, StringComparison.OrdinalIgnoreCase),
            SortColumn.RelativePath => (a, b) => NaturalStringComparer.Instance.Compare(a.RelativePath, b.RelativePath),
            _ => (a, b) => NaturalStringComparer.Instance.Compare(a.Name, b.Name),
        };

        items.Sort((a, b) =>
        {
            // Directories group before files in normal mode, regardless of direction.
            if (!IsFlattened && a.IsDirectory != b.IsDirectory)
                return a.IsDirectory ? -1 : 1;

            var result = cmp(a, b);
            if (result == 0)
                result = NaturalStringComparer.Instance.Compare(a.Name, b.Name);
            return SortDescending ? -result : result;
        });
    }
}
