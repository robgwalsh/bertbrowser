using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.App.Interop;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>The sidebar's "Drives" section: every ready drive (fixed, removable/USB,
/// network, optical) as an expandable tree, plus any connected portable devices
/// (phones/cameras) as leaf rows that open in Explorer.</summary>
public sealed class FolderTreeViewModel
{
    public ObservableCollection<ISidebarNode> Roots { get; } = new();

    public event Action<string>? DirectorySelected;

    private readonly IFileSystemService _fileSystem;
    private readonly DirSizeRepository _dirSizes;

    /// <summary>The "Show hidden items" browse setting, mirrored here because every node consults
    /// it. Defaults to off, matching <see cref="BertBrowser.App.Services.AppSettings.ShowHiddenItems"/>.</summary>
    internal bool ShowHidden { get; private set; }

    public FolderTreeViewModel(IFileSystemService fileSystem, DirSizeRepository dirSizes)
    {
        _fileSystem = fileSystem;
        _dirSizes = dirSizes;
    }

    /// <summary>Applies the "Show hidden items" setting to the whole tree. Hidden folders are kept
    /// in each node's unfiltered child list rather than discarded, so toggling this re-filters in
    /// memory — no re-enumeration, and expansion state survives a round trip.</summary>
    public void SetShowHidden(bool showHidden)
    {
        if (ShowHidden == showHidden) return;
        ShowHidden = showHidden;
        foreach (var root in Roots.OfType<DirectoryNodeViewModel>())
            root.ApplyHiddenFilter();
    }

    /// <summary>Enumerates ready drives off the UI thread and adds them as expandable roots.
    /// <see cref="IFileSystemService.GetDrives"/> filters on <c>DriveInfo.IsReady</c> and each
    /// root node's ctor probes for children and reads the volume label — all of which can block
    /// for seconds on optical/network drives, so none of it may run on the UI thread. Must be
    /// awaited on the UI thread so the nodes are added there (and before the first
    /// <see cref="RevealPathAsync"/>, which needs the roots to exist).</summary>
    public async Task LoadDrivesAsync()
    {
        var roots = await Task.Run(() => _fileSystem.GetDrives()
            .Select(drive =>
            {
                var label = string.IsNullOrEmpty(drive.VolumeLabel)
                    ? drive.Name.TrimEnd('\\')
                    : $"{drive.VolumeLabel} ({drive.Name.TrimEnd('\\')})";
                var node = new DirectoryNodeViewModel(this, drive.RootDirectory.FullName, label);
                node.SizeBytes = UsedBytes(drive); // set before the node is live; no UI thread needed
                return node;
            })
            .ToList());

        foreach (var root in roots)
            Roots.Add(root);
    }

    /// <summary>Enumerates MTP/PTP portable devices off the UI thread and appends them
    /// below the drives. Must be awaited on the UI thread so the nodes are added there.</summary>
    public async Task LoadDevicesAsync()
    {
        var devices = await Task.Run(PortableDevices.Enumerate);
        foreach (var device in devices)
            Roots.Add(new PortableDeviceNodeViewModel(device));
    }

    internal SubdirectoryPresence ProbeSubdirectories(string path) => _fileSystem.ProbeSubdirectories(path);

    // --- Sizes shown beside each name ---

    /// <summary>Fills in the size beside each of <paramref name="nodes"/> from the directory size
    /// cache, which the MFT indexer populates for every directory on every NTFS volume — so this
    /// is one indexed lookup, never a scan. Directories the cache doesn't know (non-NTFS volumes,
    /// a volume still indexing) simply show nothing rather than a wrong or stale number.</summary>
    internal async Task HydrateSizesAsync(IReadOnlyList<DirectoryNodeViewModel> nodes)
    {
        if (nodes.Count == 0) return;
        var paths = nodes.Select(n => n.FullPath).Where(p => p.Length > 0).ToList();
        if (paths.Count == 0) return;

        var cache = await Task.Run(() => _dirSizes.GetMany(paths));
        ApplySizes(nodes, cache);
    }

    /// <summary>Re-reads the size of every node the tree has loaded: drives from the volume's used
    /// space, folders from the cache. Called when a volume finishes indexing, which is when those
    /// numbers first exist — an expanded tree fills in without being collapsed and reopened.</summary>
    public async Task RefreshSizesAsync()
    {
        var roots = Roots.OfType<DirectoryNodeViewModel>().ToList();
        var loaded = new List<DirectoryNodeViewModel>();
        foreach (var root in roots)
            CollectLoaded(root, loaded);

        var rootPaths = roots.Select(r => r.FullPath).ToList();
        var childPaths = loaded.Select(n => n.FullPath).Where(p => p.Length > 0).ToList();

        // Both the DriveInfo reads (which block on network/optical volumes) and the DB query
        // stay off the UI thread.
        var (used, cache) = await Task.Run(() => (
            rootPaths.Select(UsedBytes).ToList(),
            _dirSizes.GetMany(childPaths)));

        for (var i = 0; i < roots.Count; i++)
        {
            if (used[i] is { } bytes)
                roots[i].SizeBytes = bytes;
        }
        ApplySizes(loaded, cache);
    }

    private static void ApplySizes(
        IEnumerable<DirectoryNodeViewModel> nodes, IReadOnlyDictionary<string, DirSizeResult> cache)
    {
        foreach (var node in nodes)
        {
            if (node.FullPath.Length == 0) continue;

            string key;
            try
            {
                key = PathKey.Canonicalize(node.FullPath);
            }
            catch (ArgumentException)
            {
                continue;
            }

            if (!cache.TryGetValue(key, out var result)) continue;
            node.SizeBytes = result.SizeBytes;
            node.SizeIncomplete = result.Incomplete;
        }
    }

    private static void CollectLoaded(DirectoryNodeViewModel node, List<DirectoryNodeViewModel> into)
    {
        // Walks the loaded children, not the visible ones: a subtree hidden by the "Show hidden
        // items" setting still has to be right for when the setting comes back on.
        foreach (var child in node.LoadedChildren)
        {
            into.Add(child);
            CollectLoaded(child, into);
        }
    }

    /// <summary>A drive root's size is its volume's used space — instant, and the only honest
    /// answer for a root, whose own cache entry would exclude whatever lives outside the
    /// directory tree the indexer walks.</summary>
    private static long? UsedBytes(string root)
    {
        try
        {
            return UsedBytes(new DriveInfo(root));
        }
        catch (ArgumentException) { return null; }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    private static long? UsedBytes(DriveInfo drive)
    {
        try
        {
            return drive.IsReady ? drive.TotalSize - drive.TotalFreeSpace : null;
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    /// <summary>Enumerates immediate subdirectories as <see cref="DirectoryInfo"/> objects, whose
    /// <c>Attributes</c> come pre-populated from the directory scan — so the child nodes' hidden
    /// check costs no extra per-child stat.</summary>
    internal IReadOnlyList<DirectoryInfo> GetSubdirectories(string path)
    {
        try
        {
            return new DirectoryInfo(path).EnumerateDirectories().ToList();
        }
        catch (UnauthorizedAccessException) { return Array.Empty<DirectoryInfo>(); }
        catch (IOException) { return Array.Empty<DirectoryInfo>(); }
    }

    internal void RaiseSelected(string path)
    {
        if (!_suppressSelectionEvents)
            DirectorySelected?.Invoke(path);
    }

    private bool _suppressSelectionEvents;

    /// <summary>
    /// Expands the tree down to <paramref name="path"/> (or its deepest reachable ancestor)
    /// and selects that node without raising <see cref="DirectorySelected"/>. Returns the
    /// root-to-node chain so the view can locate the container to scroll to; empty if no
    /// root covers the path.
    /// </summary>
    /// <summary>
    /// Re-reads the children of every already-expanded node for the given directories, so folders
    /// that a transfer created or removed show up without a full reload. Nodes that were never
    /// expanded still carry their placeholder and are left alone — they read from disk on first open.
    /// </summary>
    public async Task RefreshDirectoriesAsync(IEnumerable<string> directories)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var directory in directories)
        {
            if (string.IsNullOrWhiteSpace(directory)) continue;
            try
            {
                keys.Add(PathKey.Canonicalize(directory));
            }
            catch (ArgumentException)
            {
            }
        }
        if (keys.Count == 0) return;

        foreach (var node in Roots.OfType<DirectoryNodeViewModel>())
            await RefreshMatchingAsync(node, keys);
    }

    private static async Task RefreshMatchingAsync(DirectoryNodeViewModel node, HashSet<string> keys)
    {
        if (node.FullPath.Length == 0) return; // placeholder

        string key;
        try
        {
            key = PathKey.Canonicalize(node.FullPath);
        }
        catch (ArgumentException)
        {
            return;
        }

        if (keys.Contains(key))
        {
            await node.RepopulateAsync();
            return; // its subtree was just rebuilt from disk
        }

        // Only descend where a target could still live. Walks the loaded children rather than the
        // visible ones: a hidden subtree still has to be accurate for when the setting comes back on.
        if (!keys.Any(k => PathKey.IsUnder(k, key))) return;
        foreach (var child in node.LoadedChildren.ToList())
            await RefreshMatchingAsync(child, keys);
    }

    public async Task<IReadOnlyList<DirectoryNodeViewModel>> RevealPathAsync(string path)
    {
        var targetKey = PathKey.Canonicalize(path);

        // Deepest covering root wins (only directory roots can cover a path — devices can't).
        DirectoryNodeViewModel? root = null;
        var rootKey = "";
        foreach (var candidate in Roots)
        {
            if (candidate is not DirectoryNodeViewModel dir) continue;
            var key = PathKey.Canonicalize(dir.FullPath);
            if ((key == targetKey || PathKey.IsUnder(targetKey, key)) && key.Length > rootKey.Length)
            {
                root = dir;
                rootKey = key;
            }
        }
        if (root is null) return Array.Empty<DirectoryNodeViewModel>();

        var chain = new List<DirectoryNodeViewModel> { root };
        var node = root;
        var nodeKey = rootKey;
        while (nodeKey != targetKey)
        {
            node.IsExpanded = true;
            await node.EnsurePopulatedAsync(); // children load off-thread; wait before descending

            DirectoryNodeViewModel? next = null;
            foreach (var child in node.Children)
            {
                if (child.FullPath.Length == 0) continue; // unexpanded-node placeholder
                var childKey = PathKey.Canonicalize(child.FullPath);
                if (childKey == targetKey || PathKey.IsUnder(targetKey, childKey))
                {
                    next = child;
                    nodeKey = childKey;
                    break;
                }
            }
            if (next is null) break; // not in the tree (deleted/hidden) — settle for the deepest ancestor
            node = next;
            chain.Add(node);
        }

        _suppressSelectionEvents = true;
        try
        {
            node.IsSelected = true;
        }
        finally
        {
            _suppressSelectionEvents = false;
        }
        return chain;
    }
}

public sealed partial class DirectoryNodeViewModel : ObservableObject, ISidebarNode
{
    private static readonly DirectoryNodeViewModel Placeholder = new();

    private readonly FolderTreeViewModel? _tree;
    private Task? _populateTask;
    private bool _isPopulated;
    private System.Windows.Media.ImageSource? _icon;

    /// <summary>Every child read from disk, hidden ones included. <see cref="Children"/> is this
    /// list filtered by the "Show hidden items" setting, so the setting can flip without a rescan.</summary>
    private readonly List<DirectoryNodeViewModel> _allChildren = new();

    private readonly bool _hasSubdirectories;
    private readonly bool _hasVisibleSubdirectories;

    public string FullPath { get; }
    public string Name { get; }

    /// <summary>Nesting level; drives row indentation in the full-width item template.</summary>
    public int Depth { get; }

    /// <summary>Hidden folder — ghosts the icon like the file list does.</summary>
    public bool IsHidden { get; }

    /// <summary>Dimmed like Explorer when hidden.</summary>
    // 0.55, not the 0.45 this used to be: against a dark window the lower value reads as mud
    // rather than as a dimmed icon, and it still looks clearly dimmed on a light theme.
    public double IconOpacity => IsHidden ? 0.55 : 1.0;

    public System.Windows.Media.ImageSource? Icon =>
        _icon ??= FullPath.Length > 0 ? Interop.ShellIcons.GetIcon(FullPath, isDirectory: true) : null;

    /// <summary>What the tree shows under this node: the populated children minus hidden ones while
    /// "Show hidden items" is off, or the lazy-load placeholder if it hasn't been expanded yet.</summary>
    public ObservableCollection<DirectoryNodeViewModel> Children { get; } = new();

    [ObservableProperty]
    private bool _isExpanded;

    [ObservableProperty]
    private bool _isSelected;

    /// <summary>Recursive content size, shown small and dimmed to the right of the name: the
    /// cached total for a folder, the volume's used space for a drive root. Null means "not
    /// known" — the row shows nothing rather than a zero it can't stand behind.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeText))]
    private long? _sizeBytes;

    /// <summary>Some of the subtree was inaccessible when the size was computed; marked with a
    /// trailing <c>*</c> exactly as the file list's Size column does.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeText))]
    private bool _sizeIncomplete;

    public string SizeText =>
        SizeBytes is { } bytes
            ? ByteSizeFormatter.Format(bytes) + (SizeIncomplete ? " *" : "")
            : "";

    private DirectoryNodeViewModel()
    {
        FullPath = "";
        Name = "…";
    }

    /// <summary>Drive-root / general ctor: stats <paramref name="fullPath"/> for its hidden attribute.</summary>
    public DirectoryNodeViewModel(FolderTreeViewModel tree, string fullPath, string? displayName = null, int depth = 0)
        : this(tree, fullPath, IsHiddenDirectory(fullPath), displayName, depth)
    {
    }

    /// <summary>Child ctor: <paramref name="info"/> came from a directory enumeration, so its
    /// <c>Attributes</c> are already cached — the hidden check adds no per-child disk stat.</summary>
    internal DirectoryNodeViewModel(FolderTreeViewModel tree, DirectoryInfo info, int depth)
        : this(tree, info.FullName, IsHiddenDirectory(info), displayName: null, depth: depth)
    {
    }

    private DirectoryNodeViewModel(FolderTreeViewModel tree, string fullPath, bool isHidden, string? displayName, int depth)
    {
        _tree = tree;
        FullPath = fullPath;
        Depth = depth;
        var fileName = Path.GetFileName(fullPath);
        Name = displayName ?? (fileName.Length > 0 ? fileName : fullPath);
        IsHidden = isHidden;

        // Both answers come from one scan, so the expander can follow the hidden setting later
        // without touching disk again — a folder whose only subfolders are hidden must not offer
        // an expander that opens onto nothing.
        var presence = tree.ProbeSubdirectories(fullPath);
        _hasSubdirectories = presence.Any;
        _hasVisibleSubdirectories = presence.AnyVisible;
        RebuildChildren();
    }

    /// <summary>Every loaded child, hidden ones included — what a refresh walks.</summary>
    internal IReadOnlyList<DirectoryNodeViewModel> LoadedChildren => _allChildren;

    private bool ShowHidden => _tree?.ShowHidden ?? true;

    private bool IsChildVisible(DirectoryNodeViewModel child) => ShowHidden || !child.IsHidden;

    /// <summary>Re-applies the hidden filter to this node and everything already loaded beneath it.
    /// Walks <see cref="_allChildren"/>, not <see cref="Children"/>, so a subtree that was filtered
    /// out keeps its own expansion state and comes back intact.</summary>
    internal void ApplyHiddenFilter()
    {
        RebuildChildren();
        foreach (var child in _allChildren)
            child.ApplyHiddenFilter();
    }

    private void RebuildChildren()
    {
        Children.Clear();
        if (!_isPopulated)
        {
            if (ShowHidden ? _hasSubdirectories : _hasVisibleSubdirectories)
                Children.Add(Placeholder);
            return;
        }

        foreach (var child in _allChildren)
        {
            if (IsChildVisible(child))
                Children.Add(child);
        }
    }

    /// <summary>Hidden attribute for a directory path; false for drive roots and anything we can't stat.</summary>
    private static bool IsHiddenDirectory(string fullPath)
    {
        try
        {
            return new DirectoryInfo(fullPath).Attributes.HasFlag(FileAttributes.Hidden);
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Hidden attribute from an already-enumerated <see cref="DirectoryInfo"/> (no extra stat).</summary>
    private static bool IsHiddenDirectory(DirectoryInfo info)
    {
        try
        {
            return info.Attributes.HasFlag(FileAttributes.Hidden);
        }
        catch
        {
            return false;
        }
    }

    partial void OnIsExpandedChanged(bool value)
    {
        if (!value) return;

        _ = EnsurePopulatedAsync();
    }

    partial void OnIsSelectedChanged(bool value)
    {
        if (!value || _tree is null) return;

        _tree.RaiseSelected(FullPath);
    }

    /// <summary>Populates children off the UI thread on first call; later calls return the same
    /// task, so the expander binding and <see cref="FolderTreeViewModel.RevealPathAsync"/> never
    /// enumerate twice or block the UI thread on the directory scan + per-child disk probes.</summary>
    public Task EnsurePopulatedAsync()
    {
        if (_populateTask is not null) return _populateTask;
        if (_tree is null) return _populateTask = Task.CompletedTask;
        return _populateTask = PopulateAsync();
    }

    /// <summary>Re-reads this node's children from disk. A node that was never populated keeps its
    /// placeholder and is left to load on first expand.</summary>
    public Task RepopulateAsync()
    {
        if (_populateTask is null || _tree is null) return Task.CompletedTask;
        return _populateTask = PopulateAsync();
    }

    private async Task PopulateAsync()
    {
        // Enumeration plus each child node's has-children probe are disk I/O — do them off the
        // UI thread, then swap the children in on it. (The hidden-attribute check is free: it
        // reads the DirectoryInfo.Attributes already cached by the enumeration.)
        var children = await Task.Run(() => _tree!.GetSubdirectories(FullPath)
            .OrderBy(info => info.Name, Interop.NaturalStringComparer.Instance)
            .Select(info => new DirectoryNodeViewModel(_tree, info, depth: Depth + 1))
            .ToList());

        // Hidden children are kept here and filtered out of Children, not dropped: that is what
        // makes toggling "Show hidden items" free.
        _allChildren.Clear();
        _allChildren.AddRange(children);
        _isPopulated = true;
        RebuildChildren();

        // One batched lookup for the whole sibling set, after the rows are on screen. It is an
        // indexed read of a table the MFT pass already filled, so it costs a fraction of the
        // directory enumeration above.
        await _tree!.HydrateSizesAsync(children);
    }
}
