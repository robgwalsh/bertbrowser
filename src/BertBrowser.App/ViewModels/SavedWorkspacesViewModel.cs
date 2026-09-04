using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.App.ViewModels;

/// <summary>A row in the sidebar's Workspaces section.</summary>
public sealed class SavedWorkspaceItemViewModel
{
    public SavedWorkspace Model { get; }

    public string Name => Model.Name;

    public string ToolTip
    {
        get
        {
            var panes = SessionLayoutRules.CountPanes(Model.Layout);
            var tabs = SessionLayoutRules.Panes(Model.Layout).Sum(p => p.Tabs?.Count ?? 0);
            return $"{panes} pane{(panes == 1 ? "" : "s")}, {tabs} tab{(tabs == 1 ? "" : "s")}";
        }
    }

    public SavedWorkspaceItemViewModel(SavedWorkspace model) => Model = model;
}

/// <summary>The Workspaces section: pane arrangements the user stored under a name, sorted by
/// name. Keeps the list in memory so the dialog can check a name for clashes without touching the
/// database on every keystroke.</summary>
public sealed partial class SavedWorkspacesViewModel : ObservableObject
{
    private readonly ISavedWorkspaceService _service;

    public ObservableCollection<SavedWorkspaceItemViewModel> Items { get; } = new();

    [ObservableProperty]
    private bool _hasItems;

    public SavedWorkspacesViewModel(ISavedWorkspaceService service) => _service = service;

    public async Task LoadAsync()
    {
        var all = await _service.GetAllAsync();
        Items.Clear();
        foreach (var w in all)
            Items.Add(new SavedWorkspaceItemViewModel(w));
        HasItems = Items.Count > 0;
    }

    /// <summary>Whether another saved workspace already uses <paramref name="name"/>, ignoring
    /// case. <paramref name="except"/> is the one being edited, whose own name is not a clash.</summary>
    public bool IsNameTaken(string name, string? except = null) =>
        Find(name) is { } hit && !string.Equals(hit.Name, except, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Stores <paramref name="workspace"/>, replacing the row it edits (<paramref name="previousName"/>)
    /// or the row that already has its name. A rename goes first so the stored casing is the one
    /// typed — an upsert onto a NOCASE key keeps the old row's spelling. Returns false only when
    /// the rename was refused, which the dialog's own check makes rare.
    /// </summary>
    public async Task<bool> SaveAsync(SavedWorkspace workspace, string? previousName)
    {
        var replacing = previousName ?? Find(workspace.Name)?.Name;
        if (replacing is not null && !string.Equals(replacing, workspace.Name, StringComparison.Ordinal))
        {
            if (!await _service.RenameAsync(replacing, workspace.Name)) return false;
        }
        await _service.SaveAsync(workspace);

        if (replacing is not null) RemoveItem(replacing);
        InsertSorted(new SavedWorkspaceItemViewModel(workspace));
        HasItems = Items.Count > 0;
        return true;
    }

    public async Task RemoveAsync(SavedWorkspaceItemViewModel item)
    {
        await _service.RemoveAsync(item.Name);
        RemoveItem(item.Name);
        HasItems = Items.Count > 0;
    }

    private SavedWorkspaceItemViewModel? Find(string name) =>
        Items.FirstOrDefault(i => string.Equals(i.Name, name, StringComparison.OrdinalIgnoreCase));

    private void RemoveItem(string name)
    {
        if (Find(name) is { } item) Items.Remove(item);
    }

    /// <summary>Mirrors the repository's ordering: by name, ignoring case.</summary>
    private void InsertSorted(SavedWorkspaceItemViewModel item)
    {
        var i = 0;
        while (i < Items.Count && string.Compare(Items[i].Name, item.Name, StringComparison.OrdinalIgnoreCase) < 0)
            i++;
        Items.Insert(i, item);
    }
}
