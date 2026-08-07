using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.App.Interop;
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

    public FolderTreeViewModel(IFileSystemService fileSystem)
    {
        _fileSystem = fileSystem;
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
                return new DirectoryNodeViewModel(this, drive.RootDirectory.FullName, label);
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

    internal bool HasSubdirectories(string path) => _fileSystem.HasSubdirectories(path);

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

        // Only descend where a target could still live.
        if (!keys.Any(k => PathKey.IsUnder(k, key))) return;
        foreach (var child in node.Children.ToList())
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
    private System.Windows.Media.ImageSource? _icon;

    public string FullPath { get; }
    public string Name { get; }

    /// <summary>Nesting level; drives row indentation in the full-width item template.</summary>
    public int Depth { get; }

    /// <summary>Hidden folder — ghosts the icon like the file list does.</summary>
    public bool IsHidden { get; }

    /// <summary>Dimmed like Explorer when hidden.</summary>
    public double IconOpacity => IsHidden ? 0.45 : 1.0;

    public System.Windows.Media.ImageSource? Icon =>
        _icon ??= FullPath.Length > 0 ? Interop.ShellIcons.GetIcon(FullPath, isDirectory: true) : null;

    public ObservableCollection<DirectoryNodeViewModel> Children { get; } = new();

    [ObservableProperty]
    private bool _isExpanded;

    [ObservableProperty]
    private bool _isSelected;

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

        if (tree.HasSubdirectories(fullPath))
            Children.Add(Placeholder);
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

        Children.Clear();
        foreach (var child in children)
            Children.Add(child);
    }
}
