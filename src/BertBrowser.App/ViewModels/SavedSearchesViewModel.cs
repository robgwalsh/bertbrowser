using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.SavedSearches;

namespace BertBrowser.App.ViewModels;

/// <summary>A row in the sidebar's Saved searches section.</summary>
public sealed class SavedSearchItemViewModel
{
    public SavedSearch Model { get; }

    public string Name => Model.Name;
    public string Query => Model.Query;

    /// <summary>The scope in words, shown after the name and in the tooltip.</summary>
    public string ScopeText => Model.Scope switch
    {
        SavedSearchScope.Folder => $"in {Model.ScopePath}",
        SavedSearchScope.ThisPc => "this PC",
        _ => "wherever you are",
    };

    public string ToolTip => $"{Model.Query} — {ScopeText}";

    public SavedSearchItemViewModel(SavedSearch model) => Model = model;
}

/// <summary>What the save dialog opens with: a suggested name, the query, the default scope, and
/// the folder the pin option offers — null when there is none to offer, because the tab is
/// inside an archive (a pinned archive interior is refused by the rules) or has no folder.</summary>
public sealed record SavedSearchSeed(string Name, string Query, SavedSearchScope Scope, string? Folder);

/// <summary>The Saved searches section: queries the user stored under a name, sorted by name.
/// Keeps the list in memory so the dialog can check a name for clashes without touching the
/// database on every keystroke.</summary>
public sealed partial class SavedSearchesViewModel : ObservableObject
{
    private readonly ISavedSearchService _service;

    public ObservableCollection<SavedSearchItemViewModel> Items { get; } = new();

    [ObservableProperty]
    private bool _hasItems;

    public SavedSearchesViewModel(ISavedSearchService service) => _service = service;

    public async Task LoadAsync()
    {
        var all = await _service.GetAllAsync();
        Items.Clear();
        foreach (var s in all)
            Items.Add(new SavedSearchItemViewModel(s));
        HasItems = Items.Count > 0;
    }

    /// <summary>Whether another saved search already uses <paramref name="name"/>, ignoring
    /// case. <paramref name="except"/> is the one being edited, whose own name is not a clash.</summary>
    public bool IsNameTaken(string name, string? except = null) =>
        Find(name) is { } hit && !string.Equals(hit.Name, except, StringComparison.OrdinalIgnoreCase);

    /// <summary>The seed for saving what a tab is searching for right now. One method serves both
    /// boxes: typing in either empties the other, so the tab's active query is the one to keep,
    /// and which box it came from decides the default scope.</summary>
    public SavedSearchSeed SeedFor(DirectoryTabViewModel tab)
    {
        var query = tab.ActiveSearchText;
        var scope = tab.IsGlobalSearch ? SavedSearchScope.ThisPc : SavedSearchScope.CurrentFolder;
        return new SavedSearchSeed(SavedSearchRules.DefaultName(query), query, scope, PinnableFolder(tab.CurrentPath));
    }

    /// <summary>The seed for editing an existing search: its own values, with the tab's folder
    /// offered for pinning when the search has none of its own.</summary>
    public SavedSearchSeed SeedFor(SavedSearchItemViewModel item, string currentPath) =>
        new(item.Name, item.Query, item.Model.Scope, item.Model.ScopePath ?? PinnableFolder(currentPath));

    private static string? PinnableFolder(string path)
    {
        if (path.Length == 0) return null;
        return ArchivePath.Parse(path, File.Exists) is null ? path : null;
    }

    /// <summary>
    /// Stores <paramref name="search"/>, replacing the row it edits (<paramref name="previousName"/>)
    /// or the row that already has its name. A rename goes first so the stored casing is the one
    /// typed — an upsert onto a NOCASE key keeps the old row's spelling. Returns false only when
    /// the rename was refused, which the dialog's own check makes rare.
    /// </summary>
    public async Task<bool> SaveAsync(SavedSearch search, string? previousName)
    {
        var replacing = previousName ?? Find(search.Name)?.Name;
        if (replacing is not null && !string.Equals(replacing, search.Name, StringComparison.Ordinal))
        {
            if (!await _service.RenameAsync(replacing, search.Name)) return false;
        }
        await _service.SaveAsync(search);

        if (replacing is not null) RemoveItem(replacing);
        InsertSorted(new SavedSearchItemViewModel(search));
        HasItems = Items.Count > 0;
        return true;
    }

    public async Task RemoveAsync(SavedSearchItemViewModel item)
    {
        await _service.RemoveAsync(item.Name);
        RemoveItem(item.Name);
        HasItems = Items.Count > 0;
    }

    private SavedSearchItemViewModel? Find(string name) =>
        Items.FirstOrDefault(i => string.Equals(i.Name, name, StringComparison.OrdinalIgnoreCase));

    private void RemoveItem(string name)
    {
        if (Find(name) is { } item) Items.Remove(item);
    }

    /// <summary>Mirrors the repository's ordering: by name, ignoring case.</summary>
    private void InsertSorted(SavedSearchItemViewModel item)
    {
        var i = 0;
        while (i < Items.Count && string.Compare(Items[i].Name, item.Name, StringComparison.OrdinalIgnoreCase) < 0)
            i++;
        Items.Insert(i, item);
    }
}
