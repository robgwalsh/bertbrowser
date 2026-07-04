using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
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

    internal void RaiseSelected(string path) => DirectorySelected?.Invoke(path);
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
