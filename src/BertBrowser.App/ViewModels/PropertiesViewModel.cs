using System.Windows.Media;
using BertBrowser.App.Interop;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Models;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>One item the properties dialog was opened for.</summary>
public readonly record struct PropertiesTarget(string FullPath, bool IsDirectory);

/// <summary>Backing VM for <see cref="Views.PropertiesDialog"/>: fresh disk stat, the indexed
/// recursive folder size, and shell property-handler metadata. Handles a whole
/// selection — with more than one target the per-item detail collapses to aggregates and the
/// attribute checkboxes go three-state, so common bits can be flipped for everything at once.</summary>
public sealed partial class PropertiesViewModel : ObservableObject
{
    private readonly DirSizeRepository _sizeRepository;
    private readonly IReadOnlyList<PropertiesTarget> _targets;

    /// <summary>Live attributes of every target that still exists on disk, keyed by full path.
    /// Empty entries are targets that vanished; they're skipped by Apply.</summary>
    private readonly Dictionary<string, FileAttributes> _attributes = new(StringComparer.OrdinalIgnoreCase);

    public bool IsSingle => _targets.Count == 1;
    public bool IsMultiple => _targets.Count > 1;

    /// <summary>The single target's path; for a multi-selection, the first one (used for the icon).</summary>
    public string FullPath => _targets[0].FullPath;

    public bool IsDirectory => _targets[0].IsDirectory;

    /// <summary>Any directory in the selection — gates the recursive folder-size section.</summary>
    public bool HasDirectories { get; }

    public string Name { get; }
    public string Title => $"{Name} Properties";
    public string LocationDisplay { get; }

    /// <summary>Newline-separated names, shown in the multi-selection item list.</summary>
    public string ItemNamesDisplay { get; }

    public string AttributesHint => IsSingle
        ? "Applies to this item only, not folder contents."
        : $"Applies to all {_targets.Count} selected items, not folder contents.";

    public ImageSource? Icon => ShellIcons.GetIcon(_targets[0].FullPath, _targets[0].IsDirectory);

    /// <summary>True once Apply changed attributes on disk; the caller refreshes the list.</summary>
    public bool AttributesChanged { get; private set; }

    [ObservableProperty]
    private string _typeName;

    /// <summary>Total size of the selected files; folders contribute through the
    /// folder-contents section instead.</summary>
    [ObservableProperty]
    private string? _sizeDisplay;

    [ObservableProperty]
    private string _createdDisplay = "";

    [ObservableProperty]
    private string _modifiedDisplay = "";

    [ObservableProperty]
    private string _accessedDisplay = "";

    /// <summary>Three-state across the selection: null means the targets disagree, and Apply
    /// leaves that bit alone.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand))]
    private bool? _isReadOnly;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand))]
    private bool? _isHidden;

    [ObservableProperty]
    private string? _errorMessage;

    /// <summary>Nothing in the selection exists on disk any more.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ApplyAttributesCommand))]
    private bool _isMissing;

    [ObservableProperty]
    private string? _folderSizeDisplay;

    [ObservableProperty]
    private string? _folderCountsDisplay;

    [ObservableProperty]
    private string? _sizeComputedDisplay;

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
        DirSizeRepository sizeRepository)
        : this([new PropertiesTarget(fullPath, isDirectory)], sizeRepository)
    {
    }

    public PropertiesViewModel(
        IReadOnlyList<PropertiesTarget> targets,
        DirSizeRepository sizeRepository)
    {
        if (targets.Count == 0) throw new ArgumentException("At least one target is required.", nameof(targets));

        _targets = targets;
        _sizeRepository = sizeRepository;
        HasDirectories = targets.Any(t => t.IsDirectory);

        var fileCount = targets.Count(t => !t.IsDirectory);
        var dirCount = targets.Count - fileCount;

        Name = IsSingle ? DisplayName(targets[0].FullPath) : $"{targets.Count:N0} items";
        _typeName = IsSingle ? SingleTypeName(targets[0]) : SelectionTypeName(fileCount, dirCount);
        LocationDisplay = IsSingle ? targets[0].FullPath : CommonLocation(targets);
        ItemNamesDisplay = string.Join(Environment.NewLine, targets.Select(t => DisplayName(t.FullPath)));
    }

    /// <summary>Drive roots have no file name component.</summary>
    private static string DisplayName(string fullPath) =>
        Path.GetFileName(fullPath) is { Length: > 0 } name ? name : fullPath;

    private static string SingleTypeName(PropertiesTarget target) =>
        target.IsDirectory
            ? "Folder"
            : Path.GetExtension(target.FullPath) is { Length: > 1 } ext
                ? ext[1..].ToUpperInvariant() + " file"
                : "File";

    private string SelectionTypeName(int fileCount, int dirCount)
    {
        if (fileCount == 0) return Plural(dirCount, "folder");

        var extensions = _targets
            .Where(t => !t.IsDirectory)
            .Select(t => Path.GetExtension(t.FullPath).ToUpperInvariant())
            .Distinct()
            .ToList();
        var filesLabel = extensions is [{ Length: > 1 } only]
            ? Plural(fileCount, $"{only[1..]} file")
            : Plural(fileCount, "file");

        return dirCount == 0 ? filesLabel : $"{filesLabel}, {Plural(dirCount, "folder")}";
    }

    private static string Plural(int count, string singular) =>
        $"{count:N0} {singular}" + (count == 1 ? "" : "s");

    /// <summary>The folder the whole selection sits in, or a note when it spans several
    /// (search results can mix folders freely).</summary>
    private static string CommonLocation(IReadOnlyList<PropertiesTarget> targets)
    {
        var first = Path.GetDirectoryName(targets[0].FullPath);
        if (string.IsNullOrEmpty(first)) return "Multiple locations";
        return targets.All(t => string.Equals(Path.GetDirectoryName(t.FullPath), first, StringComparison.OrdinalIgnoreCase))
            ? first
            : "Multiple locations";
    }

    public async Task LoadAsync()
    {
        var stats = await Task.Run(() => _targets.Select(Stat).ToList());

        var found = new List<StatSnapshot>();
        for (var i = 0; i < _targets.Count; i++)
        {
            if (stats[i] is not { } stat) continue;
            found.Add(stat);
            _attributes[_targets[i].FullPath] = stat.Attributes;
        }

        if (found.Count == 0)
        {
            IsMissing = true;
            ErrorMessage = IsSingle
                ? "This item no longer exists on disk."
                : "None of the selected items exist on disk any more.";
            IsLoadingDetails = false;
            return;
        }
        if (found.Count < _targets.Count)
            ErrorMessage = $"{_targets.Count - found.Count} of the selected items no longer exist on disk.";

        SyncAttributeChecks();

        if (IsSingle)
        {
            var stat = found[0];
            CreatedDisplay = stat.Created.ToString("g");
            ModifiedDisplay = stat.Modified.ToString("g");
            AccessedDisplay = stat.Accessed.ToString("g");
            if (!IsDirectory)
                SizeDisplay = FormatBytes(stat.Length);
        }
        else
        {
            var fileBytes = found.Where(s => !s.IsDirectory).Sum(s => s.Length);
            SizeDisplay = FormatBytes(fileBytes) + (HasDirectories ? " — files only" : "");
        }

        await LoadCachedFolderSizeAsync();

        // Shell property handlers describe one file; there's nothing meaningful to merge.
        DetailProperties = IsSingle
            ? await Task.Run(() => ShellProperties.Read(FullPath))
            : Array.Empty<ShellProperty>();
        IsLoadingDetails = false;
    }

    private static StatSnapshot? Stat(PropertiesTarget target)
    {
        try
        {
            FileSystemInfo info = target.IsDirectory
                ? new DirectoryInfo(target.FullPath)
                : new FileInfo(target.FullPath);
            if (!info.Exists) return null;
            return new StatSnapshot(
                target.IsDirectory, info.Attributes, info.CreationTime, info.LastWriteTime, info.LastAccessTime,
                info is FileInfo file ? file.Length : 0L);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return null;
        }
    }

    private static string FormatBytes(long bytes) => $"{ByteSizeFormatter.Format(bytes)} ({bytes:N0} bytes)";

    /// <summary>Sets each checkbox to the value the whole selection shares, or indeterminate
    /// when they differ.</summary>
    private void SyncAttributeChecks()
    {
        IsReadOnly = CommonFlag(FileAttributes.ReadOnly);
        IsHidden = CommonFlag(FileAttributes.Hidden);
        ApplyAttributesCommand.NotifyCanExecuteChanged();
    }

    private bool? CommonFlag(FileAttributes flag)
    {
        bool? common = null;
        foreach (var attrs in _attributes.Values)
        {
            var set = attrs.HasFlag(flag);
            if (common is null) common = set;
            else if (common != set) return null;
        }
        return common;
    }

    // --- Recursive folder size (aggregated over every selected folder) ---

    private IEnumerable<string> SelectedDirectories => _targets.Where(t => t.IsDirectory).Select(t => t.FullPath);

    /// <summary>Shows the indexed recursive size, but only once every selected folder has one —
    /// a partial total would silently understate the selection. Nothing scans here: the rows come
    /// from the MFT pass, so an unindexed volume shows nothing rather than a stale or partial
    /// number.</summary>
    private async Task LoadCachedFolderSizeAsync()
    {
        if (!HasDirectories) return;

        var dirs = SelectedDirectories.ToList();
        var cached = await Task.Run(() => dirs.Select(d => _sizeRepository.Get(d)).ToList());
        if (cached.Any(c => c is null)) return;

        ApplySizeResults(cached.Select(c => c!).ToList());
    }

    private void ApplySizeResults(IReadOnlyList<DirSizeResult> results)
    {
        var bytes = results.Sum(r => r.SizeBytes);
        var incomplete = results.Any(r => r.Incomplete);
        var oldest = results.Min(r => r.ComputedUtc);

        FolderSizeDisplay = FormatBytes(bytes);
        FolderCountsDisplay = $"{results.Sum(r => r.FileCount):N0} files, {results.Sum(r => r.DirCount):N0} folders";
        SizeComputedDisplay = $"Computed {oldest.ToLocalTime():g}"
            + (results.Count > 1 ? $" — total across {results.Count:N0} folders" : "")
            + (incomplete ? " — may be incomplete, some folders were inaccessible" : "");
    }

    // --- Attributes ---

    private bool CanApplyAttributes =>
        !IsMissing && _attributes.Count > 0
        && (HasPendingChange(FileAttributes.ReadOnly, IsReadOnly) || HasPendingChange(FileAttributes.Hidden, IsHidden));

    private bool HasPendingChange(FileAttributes flag, bool? desired) =>
        desired is { } want && _attributes.Values.Any(a => a.HasFlag(flag) != want);

    [RelayCommand(CanExecute = nameof(CanApplyAttributes))]
    private void ApplyAttributes()
    {
        var failures = new List<string>();
        var changed = 0;

        foreach (var target in _targets)
        {
            if (!_attributes.ContainsKey(target.FullPath)) continue; // already gone; nothing to write

            try
            {
                // Re-read and touch only the bits the user set, so concurrent changes to others —
                // and bits left indeterminate across the selection — survive.
                var attrs = File.GetAttributes(target.FullPath);
                var updated = attrs;
                if (IsReadOnly is { } readOnly)
                    updated = readOnly ? updated | FileAttributes.ReadOnly : updated & ~FileAttributes.ReadOnly;
                if (IsHidden is { } hidden)
                    updated = hidden ? updated | FileAttributes.Hidden : updated & ~FileAttributes.Hidden;

                if (updated != attrs)
                {
                    File.SetAttributes(target.FullPath, updated);
                    changed++;
                }
                _attributes[target.FullPath] = updated;
            }
            catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException)
            {
                _attributes.Remove(target.FullPath);
                failures.Add($"{DisplayName(target.FullPath)}: no longer exists");
            }
            catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
            {
                failures.Add(ex is UnauthorizedAccessException
                    ? $"{DisplayName(target.FullPath)}: access denied"
                    : $"{DisplayName(target.FullPath)}: {ex.Message}");
            }
        }

        AttributesChanged |= changed > 0;
        IsMissing = _attributes.Count == 0;
        ErrorMessage = failures.Count switch
        {
            0 => null,
            _ when changed > 0 => $"Changed {changed:N0} item(s); {failures.Count:N0} failed — {failures[0]}",
            1 => $"Could not change attributes — {failures[0]}",
            _ => $"Could not change attributes on {failures.Count:N0} items — {failures[0]}",
        };

        // Re-seed the checkboxes from what is actually on disk now.
        SyncAttributeChecks();
    }

    private sealed record StatSnapshot(
        bool IsDirectory, FileAttributes Attributes,
        DateTime Created, DateTime Modified, DateTime Accessed, long Length);
}
