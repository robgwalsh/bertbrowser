using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.App.ViewModels;

/// <summary>A saved workspace as the sidebar, the title-bar dropdown and the Settings list show it.
/// Immutable: a rename or a switch replaces the row, the way the list already did for a save.</summary>
public sealed class SavedWorkspaceItemViewModel
{
    public SavedWorkspace Model { get; }

    public string Name => Model.Name;

    /// <summary>"2 panes, 5 tabs".</summary>
    public string ShapeText => SavedWorkspaceRules.ShapeText(Model.Layout);

    public string ToolTip => ShapeText;

    /// <summary>Every folder it opens, one per line, for the Settings row's tooltip.</summary>
    public string FoldersToolTip => string.Join(Environment.NewLine, SavedWorkspaceRules.Folders(Model.Layout));

    /// <summary>The folders on one line, for the Settings row itself — trimmed by the view.</summary>
    public string FoldersText => string.Join("  ·  ", SavedWorkspaceRules.Folders(Model.Layout));

    public string CreatedText => RelativeTime.Day(Model.CreatedUtc?.ToLocalTime(), DateTime.Now, missing: "Unknown");

    public string LastUsedText => RelativeTime.Day(Model.LastUsedUtc?.ToLocalTime(), DateTime.Now);

    public SavedWorkspaceItemViewModel(SavedWorkspace model) => Model = model;
}

/// <summary>The saved workspaces, sorted by name. Keeps the list in memory so the dialog can check
/// a name for clashes without touching the database on every keystroke. One instance, owned by the
/// shell, backs every place they are shown — sidebar, title bar and Settings — so a rename in one is
/// already in the others.</summary>
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

        // The row being replaced keeps its dates, as the database does; a new one was created now.
        var kept = replacing is not null ? Find(replacing)?.Model : null;
        var stored = workspace with
        {
            CreatedUtc = workspace.CreatedUtc ?? kept?.CreatedUtc ?? DateTime.UtcNow,
            LastUsedUtc = workspace.LastUsedUtc ?? kept?.LastUsedUtc,
        };

        if (replacing is not null) RemoveItem(replacing);
        InsertSorted(new SavedWorkspaceItemViewModel(stored));
        HasItems = Items.Count > 0;
        return true;
    }

    /// <summary>Renames <paramref name="item"/> and nothing else: the layout and both dates stay
    /// as they were. Returns false when another row already has the name.</summary>
    public async Task<bool> RenameAsync(SavedWorkspaceItemViewModel item, string newName)
    {
        if (string.Equals(item.Name, newName, StringComparison.Ordinal)) return true;
        if (!await _service.RenameAsync(item.Name, newName)) return false;

        RemoveItem(item.Name);
        InsertSorted(new SavedWorkspaceItemViewModel(item.Model with { Name = newName }));
        return true;
    }

    /// <summary>Records that <paramref name="item"/> was just switched to, and returns the row that
    /// now stands for it.</summary>
    public async Task<SavedWorkspaceItemViewModel> MarkUsedAsync(SavedWorkspaceItemViewModel item)
    {
        var now = DateTime.UtcNow;
        await _service.MarkUsedAsync(item.Name, now);

        var index = Items.IndexOf(item);
        var updated = new SavedWorkspaceItemViewModel(item.Model with { LastUsedUtc = now });
        if (index >= 0) Items[index] = updated;
        return updated;
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
