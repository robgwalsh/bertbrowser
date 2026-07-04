using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed class FolderTreeViewModel
{
    public ObservableCollection<DirectoryNodeViewModel> Roots { get; } = new();

    public event Action<string>? DirectorySelected;

    private readonly IFileSystemService _fileSystem;

    public FolderTreeViewModel(IFileSystemService fileSystem)
    {
        _fileSystem = fileSystem;

        foreach (var special in new[]
                 {
                     Environment.SpecialFolder.Desktop,
                     Environment.SpecialFolder.MyDocuments,
                     Environment.SpecialFolder.UserProfile,
                 })
        {
            var path = Environment.GetFolderPath(special);
            if (path.Length > 0 && Directory.Exists(path))
                Roots.Add(new DirectoryNodeViewModel(this, path, Path.GetFileName(path) is { Length: > 0 } n ? n : path));
        }

        foreach (var drive in _fileSystem.GetDrives())
        {
            var label = string.IsNullOrEmpty(drive.VolumeLabel)
                ? drive.Name.TrimEnd('\\')
                : $"{drive.VolumeLabel} ({drive.Name.TrimEnd('\\')})";
            Roots.Add(new DirectoryNodeViewModel(this, drive.RootDirectory.FullName, label));
        }
    }

    internal bool HasSubdirectories(string path) => _fileSystem.HasSubdirectories(path);

    internal IEnumerable<string> GetSubdirectories(string path)
    {
        try
        {
            return Directory.EnumerateDirectories(path).ToList();
        }
        catch (UnauthorizedAccessException) { return Array.Empty<string>(); }
        catch (IOException) { return Array.Empty<string>(); }
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
    public IReadOnlyList<DirectoryNodeViewModel> RevealPath(string path)
    {
        var targetKey = PathKey.Canonicalize(path);

        // Deepest covering root wins, e.g. Documents over its drive.
        DirectoryNodeViewModel? root = null;
        var rootKey = "";
        foreach (var candidate in Roots)
        {
            var key = PathKey.Canonicalize(candidate.FullPath);
            if ((key == targetKey || PathKey.IsUnder(targetKey, key)) && key.Length > rootKey.Length)
            {
                root = candidate;
                rootKey = key;
            }
        }
        if (root is null) return Array.Empty<DirectoryNodeViewModel>();

        var chain = new List<DirectoryNodeViewModel> { root };
        var node = root;
        var nodeKey = rootKey;
        while (nodeKey != targetKey)
        {
            node.IsExpanded = true; // populates children synchronously on first expansion

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

public sealed partial class DirectoryNodeViewModel : ObservableObject
{
    private static readonly DirectoryNodeViewModel Placeholder = new();

    private readonly FolderTreeViewModel? _tree;
    private bool _populated;
    private System.Windows.Media.ImageSource? _icon;

    public string FullPath { get; }
    public string Name { get; }

    /// <summary>Nesting level; drives row indentation in the full-width item template.</summary>
    public int Depth { get; }

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

    public DirectoryNodeViewModel(FolderTreeViewModel tree, string fullPath, string? displayName = null, int depth = 0)
    {
        _tree = tree;
        FullPath = fullPath;
        Depth = depth;
        var fileName = Path.GetFileName(fullPath);
        Name = displayName ?? (fileName.Length > 0 ? fileName : fullPath);

        if (tree.HasSubdirectories(fullPath))
            Children.Add(Placeholder);
    }

    partial void OnIsExpandedChanged(bool value)
    {
        if (value) Populate();
    }

    partial void OnIsSelectedChanged(bool value)
    {
        if (value && _tree is not null)
            _tree.RaiseSelected(FullPath);
    }

    private void Populate()
    {
        if (_populated || _tree is null) return;
        _populated = true;

        Children.Clear();
        foreach (var dir in _tree.GetSubdirectories(FullPath)
                     .OrderBy(Path.GetFileName, Interop.NaturalStringComparer.Instance))
        {
            Children.Add(new DirectoryNodeViewModel(_tree, dir, depth: Depth + 1));
        }
    }
}
