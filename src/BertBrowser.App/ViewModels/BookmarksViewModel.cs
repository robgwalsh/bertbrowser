using System.Collections.ObjectModel;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.App.Interop;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>A pinned file or directory in the sidebar's Bookmarks section.</summary>
public sealed class BookmarkItemViewModel
{
    public string FullPath { get; }
    public bool IsDirectory { get; }
    public string Name { get; }
    public ImageSource? Icon { get; }

    public BookmarkItemViewModel(string fullPath, bool isDirectory)
    {
        FullPath = fullPath;
        IsDirectory = isDirectory;
        var name = Path.GetFileName(fullPath.TrimEnd('\\'));
        Name = name.Length > 0 ? name : fullPath; // drive roots have no file name
        Icon = ShellIcons.GetIcon(fullPath, isDirectory);
    }
}

/// <summary>The Bookmarks section: user-pinned paths, ordered folders-first. Keeps an
/// in-memory set of canonical keys so the file-list context menu can label its toggle
/// (Bookmark / Remove bookmark) without hitting the database.</summary>
public sealed partial class BookmarksViewModel : ObservableObject
{
    private readonly IBookmarkService _service;
    private readonly HashSet<string> _keys = new(StringComparer.Ordinal);

    public ObservableCollection<BookmarkItemViewModel> Items { get; } = new();

    [ObservableProperty]
    private bool _hasBookmarks;

    public BookmarksViewModel(IBookmarkService service) => _service = service;

    public async Task LoadAsync()
    {
        var bookmarks = await _service.GetAllAsync();
        Items.Clear();
        _keys.Clear();
        foreach (var b in bookmarks)
        {
            Items.Add(new BookmarkItemViewModel(b.DisplayPath, b.IsDirectory));
            _keys.Add(PathKey.Canonicalize(b.DisplayPath));
        }
        HasBookmarks = Items.Count > 0;
    }

    public bool IsBookmarked(string path) => _keys.Contains(PathKey.Canonicalize(path));

    public async Task AddAsync(string path, bool isDirectory)
    {
        if (!await _service.AddAsync(path, isDirectory)) return; // already present
        _keys.Add(PathKey.Canonicalize(path));
        InsertSorted(new BookmarkItemViewModel(PathKey.NormalizeDisplay(path), isDirectory));
        HasBookmarks = Items.Count > 0;
    }

    public async Task RemoveAsync(string path)
    {
        await _service.RemoveAsync(path);
        var key = PathKey.Canonicalize(path);
        _keys.Remove(key);
        for (var i = 0; i < Items.Count; i++)
        {
            if (PathKey.Canonicalize(Items[i].FullPath) == key)
            {
                Items.RemoveAt(i);
                break;
            }
        }
        HasBookmarks = Items.Count > 0;
    }

    /// <summary>Adds or removes the bookmark; returns whether it is bookmarked afterwards.</summary>
    public async Task<bool> ToggleAsync(string path, bool isDirectory)
    {
        if (IsBookmarked(path))
        {
            await RemoveAsync(path);
            return false;
        }
        await AddAsync(path, isDirectory);
        return true;
    }

    /// <summary>Mirrors the repository's ordering: folders before files, then by name.</summary>
    private void InsertSorted(BookmarkItemViewModel item)
    {
        var i = 0;
        while (i < Items.Count && Compare(Items[i], item) < 0)
            i++;
        Items.Insert(i, item);
    }

    private static int Compare(BookmarkItemViewModel a, BookmarkItemViewModel b)
    {
        if (a.IsDirectory != b.IsDirectory)
            return a.IsDirectory ? -1 : 1; // directories first
        return string.Compare(a.FullPath, b.FullPath, StringComparison.OrdinalIgnoreCase);
    }
}
