using System.Windows.Media;
using BertBrowser.App.Interop;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Models;
using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>Backing VM for <see cref="Views.PropertiesDialog"/>: fresh disk stat, tags,
/// cached/computed recursive folder size, and shell property-handler metadata.</summary>
public sealed partial class PropertiesViewModel : ObservableObject
{
    private readonly ITagService _tagService;
    private readonly IDirectorySizeService _sizeService;
    private readonly DirSizeRepository _sizeRepository;

    private FileAttributes? _originalAttributes;
    private CancellationTokenSource? _scanCts;

    public string FullPath { get; }
    public bool IsDirectory { get; }
    public string Name { get; }
    public string Title => $"{Name} Properties";
    public ImageSource? Icon => ShellIcons.GetIcon(FullPath, IsDirectory);

    /// <summary>True once Apply changed attributes on disk; the caller refreshes the list.</summary>
    public bool AttributesChanged { get; private set; }

    [ObservableProperty]
    private string _typeName;

    /// <summary>Files only; folders use the folder-contents section.</summary>
    [ObservableProperty]
    private string? _sizeDisplay;

    [ObservableProperty]
    private string _createdDisplay = "";

    [ObservableProperty]
    private string _modifiedDisplay = "";

    [ObservableProperty]
    private string _accessedDisplay = "";

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand))]
    private bool _isReadOnly;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand))]
    private bool _isHidden;

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand), nameof(CalculateSizeCommand))]
    private bool _isMissing;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasTags))]
    private IReadOnlyList<Tag> _tags = Array.Empty<Tag>();

    public bool HasTags => Tags.Count > 0;

    [ObservableProperty]
    private string? _folderSizeDisplay;

    [ObservableProperty]
    private string? _folderCountsDisplay;

    [ObservableProperty]
    private string? _sizeComputedDisplay;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CalculateSizeCommand), nameof(CancelCalculateCommand))]
    private bool _isCalculating;

    [ObservableProperty]
    private string? _scanProgressText;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowNoMetadata))]
    private IReadOnlyList<ShellProperty> _detailProperties = Array.Empty<ShellProperty>();

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowNoMetadata))]
    private bool _isLoadingDetails = true;

    public bool ShowNoMetadata => !IsLoadingDetails && DetailProperties.Count == 0;

    public PropertiesViewModel(
        string fullPath,
        bool isDirectory,
        ITagService tagService,
        IDirectorySizeService sizeService,
        DirSizeRepository sizeRepository)
    {
        FullPath = fullPath;
        IsDirectory = isDirectory;
        _tagService = tagService;
        _sizeService = sizeService;
        _sizeRepository = sizeRepository;

        // Drive roots have no file name component.
        Name = Path.GetFileName(fullPath) is { Length: > 0 } name ? name : fullPath;
        _typeName = isDirectory
            ? "Folder"
            : Path.GetExtension(fullPath) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    public async Task LoadAsync()
    {
        var stat = await Task.Run(() =>
        {
            FileSystemInfo info = IsDirectory ? new DirectoryInfo(FullPath) : new FileInfo(FullPath);
            if (!info.Exists)
                return null;
            return new StatSnapshot(
                info.Attributes, info.CreationTime, info.LastWriteTime, info.LastAccessTime,
                info is FileInfo file ? file.Length : 0L);
        });

        if (stat is null)
        {
            IsMissing = true;
            ErrorMessage = "This item no longer exists on disk.";
            IsLoadingDetails = false;
            return;
        }

        _originalAttributes = stat.Attributes;
        IsReadOnly = stat.Attributes.HasFlag(FileAttributes.ReadOnly);
        IsHidden = stat.Attributes.HasFlag(FileAttributes.Hidden);
        CreatedDisplay = stat.Created.ToString("g");
        ModifiedDisplay = stat.Modified.ToString("g");
        AccessedDisplay = stat.Accessed.ToString("g");
        if (!IsDirectory)
            SizeDisplay = $"{ByteSizeFormatter.Format(stat.Length)} ({stat.Length:N0} bytes)";
        ApplyAttributesCommand.NotifyCanExecuteChanged();

        if (!IsDirectory)
        {
            var tagsByPath = await _tagService.GetTagsForPathsAsync(new[] { FullPath });
            if (tagsByPath.TryGetValue(PathKey.Canonicalize(FullPath), out var tags))
                Tags = tags;
        }
        else if (await Task.Run(() => _sizeRepository.Get(FullPath)) is { } cached)
        {
            ApplySizeResult(cached);
        }

        DetailProperties = await Task.Run(() => ShellProperties.Read(FullPath));
        IsLoadingDetails = false;
    }

    private bool CanCalculateSize => IsDirectory && !IsMissing && !IsCalculating;

    [RelayCommand(CanExecute = nameof(CanCalculateSize))]
    private async Task CalculateSizeAsync()
    {
        _scanCts = new CancellationTokenSource();
        IsCalculating = true;
        try
        {
            var progress = new Progress<DirScanProgress>(p =>
                ScanProgressText = $"Scanning… {p.DirectoriesScanned} folders, {ByteSizeFormatter.Format(p.BytesSoFar)}");
            var result = await _sizeService.ComputeAsync(FullPath, _scanCts.Token, progress);
            if (result is not null)
                ApplySizeResult(result);
        }
        catch (Exception ex)
        {
            ErrorMessage = $"Size scan failed: {ex.Message}";
        }
        finally
        {
            IsCalculating = false;
            ScanProgressText = null;
            _scanCts?.Dispose();
            _scanCts = null;
        }
    }

    [RelayCommand(CanExecute = nameof(IsCalculating))]
    private void CancelCalculate() => _scanCts?.Cancel();

    private void ApplySizeResult(DirSizeResult result)
    {
        FolderSizeDisplay = $"{ByteSizeFormatter.Format(result.SizeBytes)} ({result.SizeBytes:N0} bytes)";
        FolderCountsDisplay = $"{result.FileCount:N0} files, {result.DirCount:N0} folders";
        SizeComputedDisplay = $"Computed {result.ComputedUtc.ToLocalTime():g}"
            + (result.Incomplete ? " — may be incomplete, some folders were inaccessible" : "");
    }

    private bool CanApplyAttributes =>
        !IsMissing && _originalAttributes is { } attrs
        && (IsReadOnly != attrs.HasFlag(FileAttributes.ReadOnly) || IsHidden != attrs.HasFlag(FileAttributes.Hidden));

    [RelayCommand(CanExecute = nameof(CanApplyAttributes))]
    private void ApplyAttributes()
    {
        try
        {
            // Re-read and touch only our two bits so concurrent changes to others survive.
            var attrs = File.GetAttributes(FullPath);
            attrs = IsReadOnly ? attrs | FileAttributes.ReadOnly : attrs & ~FileAttributes.ReadOnly;
            attrs = IsHidden ? attrs | FileAttributes.Hidden : attrs & ~FileAttributes.Hidden;
            File.SetAttributes(FullPath, attrs);
            _originalAttributes = attrs;
            AttributesChanged = true;
            ErrorMessage = null;
        }
        catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException)
        {
            IsMissing = true;
            ErrorMessage = "This item no longer exists on disk.";
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
        {
            ErrorMessage = ex is UnauthorizedAccessException
                ? "Access denied — you don't have permission to change these attributes."
                : $"Could not change attributes: {ex.Message}";
            if (_originalAttributes is { } original)
            {
                IsReadOnly = original.HasFlag(FileAttributes.ReadOnly);
                IsHidden = original.HasFlag(FileAttributes.Hidden);
            }
        }
        ApplyAttributesCommand.NotifyCanExecuteChanged();
    }

    /// <summary>Called when the dialog closes so an in-flight scan doesn't outlive it.</summary>
    public void CancelPendingWork() => _scanCts?.Cancel();

    private sealed record StatSnapshot(
        FileAttributes Attributes, DateTime Created, DateTime Modified, DateTime Accessed, long Length);
}
