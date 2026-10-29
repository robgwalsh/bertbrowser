using BertBrowser.Core.Services.CommandBars;

namespace BertBrowser.App.Services.Commands;

/// <summary>
/// The menu bar and toolbars in force, the arrangements saved under a name, and the only thing that
/// writes either. The window draws <see cref="Current"/>; the settings page, the View menu and the
/// palette all change it through here.
/// </summary>
/// <remarks>
/// Written at once, like the keymap and for the same reason: an arrangement can be recalled from a
/// menu or the palette while the settings page is open, and a page that wrote its own copy on a
/// debounce would put the old one back.
/// </remarks>
public sealed class CommandBarService
{
    private readonly AppSettings _settings;

    public CommandBarService(AppSettings settings)
    {
        _settings = settings;
        Current = CommandBarRules.Clean(settings.CommandBars);
    }

    public CommandBarSet Current { get; private set; }

    public IReadOnlyList<SavedCommandBarSet> Saved => _settings.SavedCommandBars;

    /// <summary>Raised after the arrangement in force, or the saved list, changes.</summary>
    public event EventHandler? Changed;

    public bool IsDefault => CommandBarRules.IsDefault(Current);

    /// <summary>The saved arrangement the current one is, or null when it is none of them.</summary>
    public string? CurrentName => SavedCommandBarRules.NameMatching(Saved, Current);

    /// <summary>Whether replacing the current arrangement would lose something kept nowhere else.</summary>
    public bool ReplacingLosesWork => SavedCommandBarRules.NeedsConfirmBeforeReplace(Current, Saved);

    public bool IsNameTaken(string name, string? except = null) =>
        Saved.Any(s => string.Equals(s.Name, name, StringComparison.OrdinalIgnoreCase) &&
                       !string.Equals(s.Name, except, StringComparison.OrdinalIgnoreCase));

    /// <summary>Puts an arrangement in force. One that equals what ships is stored as nothing at
    /// all, so somebody who dragged a button and dragged it back is on the default again.</summary>
    public void Set(CommandBarSet set)
    {
        var clean = CommandBarRules.Clean(set) with { Revision = CommandBarDefaults.Revision };
        if (CommandBarRules.Same(clean, Current)) return;

        Current = CommandBarRules.IsDefault(clean) ? CommandBarDefaults.Default : clean;
        _settings.CommandBars = ReferenceEquals(Current, CommandBarDefaults.Default) ? null : Current;
        Commit();
    }

    public void Reset() => Set(CommandBarDefaults.Default);

    /// <summary>Saves the current arrangement under a name, replacing one already there.</summary>
    public void SaveAs(string name)
    {
        _settings.SavedCommandBars = [.. SavedCommandBarRules.Upsert(Saved, new SavedCommandBarSet(name, Current))];
        Commit();
    }

    /// <summary>Recalls a saved arrangement. False when there is none by that name.</summary>
    public bool Apply(string name)
    {
        if (SavedCommandBarRules.Find(Saved, name) is not { } saved) return false;
        Set(saved.Set);
        return true;
    }

    /// <summary>False when the new name belongs to another arrangement.</summary>
    public bool Rename(string oldName, string newName)
    {
        if (SavedCommandBarRules.Rename(Saved, oldName, newName) is not { } renamed) return false;
        _settings.SavedCommandBars = [.. renamed];
        Commit();
        return true;
    }

    public void Remove(string name)
    {
        _settings.SavedCommandBars = [.. SavedCommandBarRules.Remove(Saved, name)];
        Commit();
    }

    private void Commit()
    {
        _settings.Save();
        Changed?.Invoke(this, EventArgs.Empty);
    }
}
