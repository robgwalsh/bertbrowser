using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.Services.Commands;

/// <summary>
/// The shortcuts in force, and the only thing that writes them. Everything that shows a gesture —
/// a menu, a tooltip, the palette, the Keyboard page — reads <see cref="Current"/>, and everything
/// that changes one comes through here.
/// </summary>
/// <remarks>
/// A change is written at once, like a theme change, rather than on the settings page's debounce:
/// a shortcut can be rebound from the palette while that page is open, and a page that wrote its
/// own copy 400 ms later would put the old binding back.
/// </remarks>
public sealed class KeymapService
{
    /// <summary>The id a custom command is bound under: this prefix and its own id.</summary>
    public const string CustomPrefix = "custom:";

    private readonly AppSettings _settings;

    public KeymapService(AppSettings settings)
    {
        _settings = settings;
        Current = Resolve();
    }

    public Keymap Current { get; private set; }

    /// <summary>Raised after any change to what a key does, including a custom command arriving
    /// or leaving.</summary>
    public event EventHandler? Changed;

    public static string CustomId(string commandId) => CustomPrefix + commandId;

    /// <summary>Everything a shortcut can be given to: the catalogue, then the user's own
    /// commands, which ship with none and act on the active tab like any other browser command.</summary>
    public IReadOnlyList<Bindable> Bindables() =>
    [
        .. KeymapRules.CatalogBindables(),
        .. _settings.CustomCommands
            .DistinctBy(c => c.Id)
            .Select(c => new Bindable(CustomId(c.Id), CommandContext.Browser, [])),
    ];

    public string GestureText(string id) => Current.GestureText(id);

    /// <summary>Gives a command exactly these shortcuts, taking each from whoever had it.</summary>
    public void SetChords(string id, IReadOnlyList<KeyChord> chords) =>
        Store(KeymapRules.WithChords(Bindables(), Overrides(), id, chords));

    /// <summary>Puts one command back to the shortcuts it ships with.</summary>
    public void Reset(string id) => Store(KeymapRules.WithDefaults(Bindables(), Overrides(), id));

    /// <summary>
    /// Whether this command's shortcuts differ from the ones it ships with — by what is in force,
    /// not by whether it has an entry of its own. A command whose default was given to another
    /// has no entry and no key, and that is a change Reset has to be able to undo.
    /// </summary>
    public bool IsChanged(string id) =>
        !Current.ChordsFor(id).SequenceEqual(Bindables().FirstOrDefault(b => b.Id == id).Defaults ?? []);

    public void ResetAll()
    {
        _settings.KeyBindings = null;
        _settings.Save();
        Refresh();
    }

    /// <summary>Re-reads the settings: the custom commands may have changed under the keymap.</summary>
    public void Refresh()
    {
        Current = Resolve();
        Changed?.Invoke(this, EventArgs.Empty);
    }

    private IReadOnlyDictionary<string, IReadOnlyList<string>>? Overrides() =>
        _settings.KeyBindings?.ToDictionary(p => p.Key, p => (IReadOnlyList<string>)p.Value, StringComparer.Ordinal);

    private Keymap Resolve() => KeymapRules.Resolve(Bindables(), Overrides());

    private void Store(IReadOnlyDictionary<string, IReadOnlyList<string>> overrides)
    {
        _settings.KeyBindings = overrides.Count == 0
            ? null
            : overrides.ToDictionary(p => p.Key, p => p.Value.ToList(), StringComparer.Ordinal);
        _settings.Save();
        Refresh();
    }
}
