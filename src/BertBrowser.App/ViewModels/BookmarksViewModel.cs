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

    /// <summary>Hidden target — filtered/dimmed like the directory views.</summary>
    public bool IsHidden { get; }

    /// <summary>Ghosted like Explorer when hidden.</summary>
    public double IconOpacity => IsHidden ? 0.45 : 1.0;

    public BookmarkItemViewModel(string fullPath, bool isDirectory)
    {
        FullPath = fullPath;
        IsDirectory = isDirectory;
        var name = Path.GetFileName(fullPath.TrimEnd('\\'));
        Name = name.Length > 0 ? name : fullPath; // drive roots have no file name
        Icon = ShellIcons.GetIcon(fullPath, isDirectory);
        IsHidden = IsHiddenEntry(fullPath);
    }

    /// <summary>Hidden attribute of the bookmark's target; false for missing or unstattable paths.</summary>
    private static bool IsHiddenEntry(string fullPath)
    {
        try
        {
            return File.GetAttributes(fullPath).HasFlag(FileAttributes.Hidden);
        }
        catch
        {
            return false;
        }
    }
}

/// <summary>The Bookmarks section: user-pinned paths, ordered folders-first. Keeps an
/// in-memory set of canonical keys so the file-list context menu can label its toggle
/// (Bookmark / Remove bookmark) without hitting the database.</summary>
public sealed partial class BookmarksViewModel : ObservableObject
{
    private readonly IBookmarkService _service;
    private readonly HashSet<string> _keys = new(StringComparer.Ordinal);

    /// <summary>Every bookmark, sorted folders-first then by name — the source the visible
    /// <see cref="Items"/> is filtered from. Hidden bookmarks live here even when filtered out.</summary>
    private readonly List<BookmarkItemViewModel> _all = new();
    private bool _showHidden;

    /// <summary>The bookmarks actually shown in the sidebar: <see cref="_all"/> minus hidden
    /// entries when the "Show hidden items" setting is off.</summary>
    public ObservableCollection<BookmarkItemViewModel> Items { get; } = new();

    [ObservableProperty]
    private bool _hasBookmarks;

    public BookmarksViewModel(IBookmarkService service) => _service = service;

    /// <summary>Matches the directory views' "Show hidden items" setting; re-filters the visible
    /// list in place without re-querying.</summary>
    public void SetShowHidden(bool showHidden)
    {
        if (_showHidden == showHidden) return;
        _showHidden = showHidden;
        RebuildVisible();
    }

    public async Task LoadAsync()
    {
        var bookmarks = await _service.GetAllAsync();
        _all.Clear();
        _keys.Clear();
        foreach (var b in bookmarks)
        {
            _all.Add(new BookmarkItemViewModel(b.DisplayPath, b.IsDirectory));
            _keys.Add(PathKey.Canonicalize(b.DisplayPath));
        }
        RebuildVisible();
    }

    public bool IsBookmarked(string path) => _keys.Contains(PathKey.Canonicalize(path));

    public async Task AddAsync(string path, bool isDirectory)
    {
        if (!await _service.AddAsync(path, isDirectory)) return; // already present
        _keys.Add(PathKey.Canonicalize(path));
        var item = new BookmarkItemViewModel(PathKey.NormalizeDisplay(path), isDirectory);
        InsertSorted(_all, item);
        if (IsVisible(item))
            InsertSorted(Items, item);
        HasBookmarks = Items.Count > 0;
    }

    public async Task RemoveAsync(string path)
    {
        await _service.RemoveAsync(path);
        var key = PathKey.Canonicalize(path);
        _keys.Remove(key);
        RemoveByKey(_all, key);
        RemoveByKey(Items, key);
        HasBookmarks = Items.Count > 0;
    }

    private bool IsVisible(BookmarkItemViewModel item) => _showHidden || !item.IsHidden;

    /// <summary>Refills <see cref="Items"/> from <see cref="_all"/> honoring the hidden filter.</summary>
    private void RebuildVisible()
    {
        Items.Clear();
        foreach (var item in _all)
        {
            if (IsVisible(item))
                Items.Add(item);
        }
        HasBookmarks = Items.Count > 0;
    }

    private static void RemoveByKey(IList<BookmarkItemViewModel> list, string key)
    {
        for (var i = 0; i < list.Count; i++)
        {
            if (PathKey.Canonicalize(list[i].FullPath) == key)
            {
                list.RemoveAt(i);
                return;
            }
        }
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
    private static void InsertSorted(IList<BookmarkItemViewModel> list, BookmarkItemViewModel item)
    {
        var i = 0;
        while (i < list.Count && Compare(list[i], item) < 0)
            i++;
        list.Insert(i, item);
    }

    private static int Compare(BookmarkItemViewModel a, BookmarkItemViewModel b)
    {
        if (a.IsDirectory != b.IsDirectory)
            return a.IsDirectory ? -1 : 1; // directories first
        return string.Compare(a.FullPath, b.FullPath, StringComparison.OrdinalIgnoreCase);
    }
}
