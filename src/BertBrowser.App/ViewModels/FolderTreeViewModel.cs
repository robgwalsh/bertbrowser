using System.Collections.ObjectModel;
using System.Windows.Threading;
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
    /// <summary>Every drive/device, in load order — never reordered. The stable source other
    /// consumers (the Cards view, hidden-filter application, path lookups) bind or iterate over,
    /// so a card list stays a fixed overview regardless of what's being browsed.</summary>
    public ObservableCollection<ISidebarNode> Roots { get; } = new();

    /// <summary>Mirrors <see cref="Roots"/> for the tree's own presentation, where
    /// <see cref="PromoteRoot"/> is allowed to move the browsed drive/device to the top — kept
    /// separate so that reordering for the tree's "anchor at top" behavior doesn't leak into
    /// <see cref="Roots"/> and reorder the Cards view underneath it.</summary>
    public ObservableCollection<ISidebarNode> TreeRoots { get; } = new();

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

        // No selection guard needed: ApplyHiddenFilter diffs the rows rather than clearing them,
        // so a folder that is staying keeps its container — and with it the selection.
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
                // Set before the node is live; no UI thread needed.
                node.SizeBytes = UsedBytes(drive);
                node.TotalBytes = TotalBytes(drive);
                return node;
            })
            .ToList());

        foreach (var root in roots)
        {
            Roots.Add(root);
            TreeRoots.Add(root);
        }
    }

    /// <summary>Enumerates MTP/PTP portable devices off the UI thread and appends them
    /// below the drives. Must be awaited on the UI thread so the nodes are added there.</summary>
    public async Task LoadDevicesAsync()
    {
        var devices = await Task.Run(PortableDevices.Enumerate);
        foreach (var device in devices)
        {
            var node = new PortableDeviceNodeViewModel(device);
            Roots.Add(node);
            TreeRoots.Add(node);
        }
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
        var (used, total, cache) = await Task.Run(() => (
            rootPaths.Select(UsedBytes).ToList(),
            rootPaths.Select(TotalBytes).ToList(),
            _dirSizes.GetMany(childPaths)));

        for (var i = 0; i < roots.Count; i++)
        {
            if (used[i] is { } bytes)
                roots[i].SizeBytes = bytes;
            if (total[i] is { } totalBytes)
                roots[i].TotalBytes = totalBytes;
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

    /// <summary>A drive root's total capacity, for the Cards view's used-space bar. Same
    /// best-effort/never-throw shape as <see cref="UsedBytes(string)"/>.</summary>
    private static long? TotalBytes(string root)
    {
        try
        {
            return TotalBytes(new DriveInfo(root));
        }
        catch (ArgumentException) { return null; }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    private static long? TotalBytes(DriveInfo drive)
    {
        try
        {
            return drive.IsReady ? drive.TotalSize : null;
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    /// <summary>Enumerates immediate subdirectories, whose <c>Attributes</c> come pre-populated
    /// from the directory scan — so the child nodes' hidden check costs no extra per-child stat.
    /// </summary>
    /// <remarks>
    /// Through <see cref="IFileSystemService"/> rather than <c>DirectoryInfo</c> directly, so this
    /// stops being the one listing path in the app that bypasses the seam. Nothing about the tree
    /// depends on that today; it is what lets a decorating lister be reached from here at all.
    /// The seam throws where this must not, so the catch set stays exactly what it was.
    /// </remarks>
    internal IReadOnlyList<FileEntry> GetSubdirectories(string path)
    {
        try
        {
            return _fileSystem.ListDirectory(path).Where(e => e.IsDirectory).ToList();
        }
        catch (UnauthorizedAccessException) { return Array.Empty<FileEntry>(); }
        catch (IOException) { return Array.Empty<FileEntry>(); }
    }

    /// <summary>
    /// A node became the selected one. Announced only when it was not this class that did it.
    /// </summary>
    /// <remarks>
    /// Two things have to be filtered out, and the second is the subtle one. A selection this class
    /// assigns is caught by <see cref="_suppressSelectionEvents"/> — but WPF <b>echoes that
    /// assignment back</b> through the container a layout pass later, long after the guard has
    /// been released, and that echo is indistinguishable from a click. So the node is remembered
    /// and its next announcement swallowed, once. The cost is at most one ignored click, on the
    /// row the tree had just selected by itself; the alternative is the tab silently navigating to
    /// wherever the tree happened to settle, which is what it used to do on startup whenever the
    /// current folder was somewhere the tree could not reveal (anything under <c>AppData</c>).
    /// </remarks>
    internal void NoteSelected(DirectoryNodeViewModel node)
    {
        if (_suppressSelectionEvents > 0)
        {
            _selfSelected = node;
            return;
        }

        var echo = ReferenceEquals(node, _selfSelected);
        _selfSelected = null;
        if (echo) return;

        DirectorySelected?.Invoke(node.FullPath);
    }

    /// <summary>The node this class last selected itself, until its echo has been accounted for.</summary>
    private DirectoryNodeViewModel? _selfSelected;

    /// <summary>Nesting depth of <see cref="Rebuilding"/> / <see cref="RevealPathAsync"/>; a count
    /// rather than a flag because a rebuild can await a repopulate that suppresses in its turn, and
    /// the inner one finishing must not un-suppress the outer.</summary>
    private int _suppressSelectionEvents;

    /// <summary>
    /// Rebuilds the tree's rows without the churn being mistaken for a click.
    /// </summary>
    /// <remarks>
    /// <para>
    /// A row that is replaced takes its <c>TreeViewItem</c> with it, and WPF answers the removal of
    /// the selected container by selecting its parent — which this tree reports as a navigation.
    /// So a refresh after a move or a delete walked the active tab up to the drive root.
    /// <see cref="DirectoryNodeViewModel.RebuildChildren"/> is what keeps the *filter* out of this
    /// (it diffs rather than clearing, so no container is disturbed); a repopulate genuinely builds
    /// new child objects, and this is the guard for that.
    /// </para>
    /// <para>
    /// <b>The suppression has to outlive the rebuild.</b> A collection change only raises a
    /// notification; WPF tears the containers down and fixes the selection up during the
    /// <em>next layout pass</em>, so a guard that ends when the method returns catches nothing at
    /// all — and looks like it works, because an assertion made straight afterwards runs before
    /// the stray selection has happened. Hence the deferral to
    /// <see cref="DispatcherPriority.Loaded"/>, which is after layout.
    /// </para>
    /// <para>
    /// Deliberately no attempt to *restore* the selection here. Putting it back by hand means
    /// assigning <c>IsSelected</c>, and WPF can flip that property false and true again a layout
    /// pass later as containers are regenerated — after the suppression has ended, which is a
    /// navigation to wherever the tree had settled. That cost a real bug: with the tab in a folder
    /// the tree could not reveal (anything under <c>AppData</c>, which is hidden), the restore
    /// re-announced the deepest reachable ancestor and the tab jumped there. A refresh that
    /// replaces the selected row therefore leaves the tree unhighlighted until the next reveal,
    /// which is the cheap half of the trade.
    /// </para>
    /// </remarks>
    private async Task RebuildingAsync(Func<Task> rebuild)
    {
        var dispatcher = Dispatcher.CurrentDispatcher;

        _suppressSelectionEvents++;
        try
        {
            await rebuild();
        }
        finally
        {
            _ = dispatcher.BeginInvoke(
                DispatcherPriority.Loaded, () => _suppressSelectionEvents--);
        }
    }

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

        // A repopulate replaces the child nodes wholesale, so the selected row's container goes
        // with them — see Rebuilding. Without this, a move or a delete walked the active tab up to
        // the drive root.
        await RebuildingAsync(async () =>
        {
            foreach (var node in Roots.OfType<DirectoryNodeViewModel>())
                await RefreshMatchingAsync(node, keys);
        });
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

    /// <summary>
    /// Moves the drive/device currently being browsed to the top of <see cref="Roots"/> and
    /// collapses every other one, so it stays in view without depending on scroll position.
    /// </summary>
    /// <remarks>
    /// <c>Roots.Move</c> raises a <c>Move</c> collection change rather than a remove/insert pair,
    /// which is what lets the container the tree already built for this node relocate instead of
    /// being torn down and rebuilt — the same concern <see cref="Sync"/> exists for on the child
    /// level. Only other <em>drives</em> are collapsed: a portable device has no children to
    /// collapse, and collapsing it would do nothing but strip its (nonexistent) expander.
    /// </remarks>
    private void PromoteRoot(DirectoryNodeViewModel root)
    {
        var index = TreeRoots.IndexOf(root);
        if (index > 0) TreeRoots.Move(index, 0);

        var toCollapse = TreeRoots.OfType<DirectoryNodeViewModel>()
            .Where(other => !ReferenceEquals(other, root) && other.IsExpanded)
            .ToList();
        if (toCollapse.Count == 0) return;

        // Collapsing a sibling that has a selected descendant tears that descendant's container
        // down on the next layout pass, and WPF answers by reselecting the collapsed root itself
        // (the same hazard RebuildingAsync guards against for a repopulate). Left unguarded, that
        // reselection re-announces as a navigation to the sibling, which promotes it right back
        // and collapses this one in turn — C: and D: trading the top spot forever. The suppression
        // has to outlive layout, hence the deferral to DispatcherPriority.Loaded.
        var dispatcher = Dispatcher.CurrentDispatcher;
        _suppressSelectionEvents++;
        foreach (var other in toCollapse)
            other.IsExpanded = false;
        _ = dispatcher.BeginInvoke(DispatcherPriority.Loaded, () => _suppressSelectionEvents--);
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

        PromoteRoot(root);

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

        _suppressSelectionEvents++;
        try
        {
            node.IsSelected = true;
        }
        finally
        {
            _suppressSelectionEvents--;
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
    [NotifyPropertyChangedFor(nameof(UsedFraction))]
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

    /// <summary>A drive root's total capacity; null for a plain folder (used space alone isn't a
    /// fraction of anything there) and for a drive whose capacity couldn't be read.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(UsedFraction))]
    private long? _totalBytes;

    /// <summary>Used space as a 0..1 fraction of <see cref="TotalBytes"/>, for the Cards view's
    /// used-space bar. Null — rather than 0 — when either half isn't known, so the bar renders
    /// empty instead of claiming a drive is unused.</summary>
    public double? UsedFraction =>
        SizeBytes is { } used && TotalBytes is { } total && total > 0 ? (double)used / total : null;

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

    /// <summary>Child ctor: <paramref name="entry"/> came from a directory enumeration, so its
    /// <c>Attributes</c> are already cached — the hidden check adds no per-child disk stat.</summary>
    internal DirectoryNodeViewModel(FolderTreeViewModel tree, FileEntry entry, int depth)
        : this(tree, entry.FullPath, entry.Attributes.HasFlag(FileAttributes.Hidden),
               displayName: null, depth: depth)
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

    /// <summary>
    /// Brings <see cref="Children"/> in line with the filter, in place.
    /// </summary>
    /// <remarks>
    /// <b>Never <c>Clear()</c>.</b> Clearing tears down every <c>TreeViewItem</c> below this node,
    /// and WPF answers the removal of the selected one by selecting its parent — which this tree
    /// reports as a navigation, so toggling "Show hidden items" walked the active tab to the drive
    /// root. Removing and inserting only what actually changed leaves every row that is staying
    /// exactly where it is, container and selection included.
    /// </remarks>
    private void RebuildChildren()
    {
        var wanted = new List<DirectoryNodeViewModel>();

        if (!_isPopulated)
        {
            if (ShowHidden ? _hasSubdirectories : _hasVisibleSubdirectories)
                wanted.Add(Placeholder);
        }
        else
        {
            foreach (var child in _allChildren)
                if (IsChildVisible(child))
                    wanted.Add(child);
        }

        Sync(Children, wanted);
    }

    /// <summary>
    /// Makes <paramref name="current"/> equal <paramref name="wanted"/> by removing and inserting,
    /// never by replacing.
    /// </summary>
    /// <remarks>
    /// Both lists come from the same source in the same order, so once what is leaving has gone,
    /// what remains is a subsequence of <paramref name="wanted"/> and a single forward pass can
    /// place the rest.
    /// </remarks>
    private static void Sync(
        ObservableCollection<DirectoryNodeViewModel> current, List<DirectoryNodeViewModel> wanted)
    {
        var keep = new HashSet<DirectoryNodeViewModel>(wanted);
        for (var i = current.Count - 1; i >= 0; i--)
            if (!keep.Contains(current[i]))
                current.RemoveAt(i);

        for (var i = 0; i < wanted.Count; i++)
            if (i >= current.Count || !ReferenceEquals(current[i], wanted[i]))
                current.Insert(i, wanted[i]);
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


    partial void OnIsExpandedChanged(bool value)
    {
        if (!value) return;

        _ = EnsurePopulatedAsync();
    }

    partial void OnIsSelectedChanged(bool value)
    {
        if (!value || _tree is null) return;

        _tree.NoteSelected(this);
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
        // reads the attributes already cached by the enumeration.)
        var children = await Task.Run(() => _tree!.GetSubdirectories(FullPath)
            .OrderBy(entry => entry.Name, Interop.NaturalStringComparer.Instance)
            .Select(entry => new DirectoryNodeViewModel(_tree, entry, depth: Depth + 1))
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
