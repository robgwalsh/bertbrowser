using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.ViewModels;

/// <summary>One line of the Keyboard page: a category heading, or a command and its shortcuts.</summary>
public sealed class KeyBindingRowViewModel
{
    private KeyBindingRowViewModel(string name) => Name = name;

    public static KeyBindingRowViewModel Heading(string category) => new(category) { IsHeading = true };

    public static KeyBindingRowViewModel For(
        string id, string name, string category, string? iconKey, CommandContext context,
        IReadOnlyList<KeyChord> chords, IReadOnlyList<KeyChord> defaults, bool changed) =>
        new(name)
        {
            Id = id,
            Category = category,
            IconKey = iconKey,
            Context = context,
            Chords = [.. chords.Select(c => c.ToString())],
            IsChanged = changed,
            DefaultText = defaults.Count == 0 ? "Ships with no shortcut" : $"Ships as {string.Join(", ", defaults)}",
        };

    public string Id { get; private init; } = "";

    public string Name { get; }

    public string Category { get; private init; } = "";

    public bool IsHeading { get; private init; }

    public string? IconKey { get; private init; }

    public CommandContext Context { get; private init; }

    /// <summary>The shortcuts in force, the one menus print first.</summary>
    public IReadOnlyList<string> Chords { get; private init; } = [];

    public bool HasChords => Chords.Count > 0;

    /// <summary>Whether these differ from what the command ships with.</summary>
    public bool IsChanged { get; private init; }

    /// <summary>What Reset would put back, for the changed marker's tooltip.</summary>
    public string DefaultText { get; private init; } = "";

    /// <summary>Where the shortcut is live, said only when it is not simply "while the folders
    /// are showing" — the two cases that explain a key seeming not to work.</summary>
    public string Where => Context switch
    {
        CommandContext.FileList => "in the file list",
        CommandContext.App => "anywhere",
        _ => "",
    };

    public Geometry? Icon => IconKey is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;
}

/// <summary>
/// The Keyboard page: every command with the keys that run it, and a recorder that takes a new
/// shortcut by having it pressed.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing here goes through <see cref="Apply"/>.</b> A shortcut is written the moment it is
/// assigned, by <see cref="KeymapService"/>, which is the only writer the keymap has — the same
/// shortcut can be changed from the command palette while this page is open, and a page that wrote
/// its own copy on a debounce would put the old one back.
/// </para>
/// <para>
/// What may be bound, and what happens when a chord is already somebody's, is
/// <see cref="KeymapRules"/>'. This page only asks: a refused chord is said to be refused and the
/// recorder keeps listening; a taken one is offered for reassignment by name, never taken silently.
/// </para>
/// </remarks>
public sealed partial class SettingsViewModel
{
    private readonly KeymapService? _keymap;

    /// <summary>Whether the recorder is adding a shortcut to the ones the command has, rather
    /// than replacing them.</summary>
    private bool _recordingAdds;

    /// <summary>The chord pressed while recording that belongs to another command, waiting on
    /// Reassign.</summary>
    private KeyChord? _pendingChord;

    public ObservableCollection<KeyBindingRowViewModel> KeyRows { get; } = [];

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ChangeKeyCommand), nameof(AddKeyCommand),
        nameof(RemoveKeysCommand), nameof(ResetKeyCommand))]
    private KeyBindingRowViewModel? _selectedKeyRow;

    /// <summary>Narrows the list: part of a name, a category, or a shortcut ("ctrl+shift").</summary>
    [ObservableProperty]
    private string _keyFilter = "";

    /// <summary>True while the next key press is being taken as a shortcut rather than acted on.</summary>
    [ObservableProperty]
    private bool _isRecordingKey;

    /// <summary>True while the next key press is being looked up rather than assigned.</summary>
    [ObservableProperty]
    private bool _isFindingKey;

    /// <summary>What the recorder strip says.</summary>
    [ObservableProperty]
    private string _recorderText = "";

    /// <summary>Whether the strip is offering to take a chord from the command that has it.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ReassignKeyCommand))]
    private bool _canReassignKey;

    /// <summary>True while either mode is listening, which is what tells the window's own key
    /// dispatcher to stand aside (<c>FocusState.Recording</c>).</summary>
    public bool IsCapturingKey => IsRecordingKey || IsFindingKey;

    /// <summary>Whether the strip under the list is saying something — listening, or waiting on
    /// Reassign — rather than showing the selected command's buttons.</summary>
    public bool ShowsRecorder => IsCapturingKey || CanReassignKey;

    public bool CanEditKeys => _keymap is not null;

    public bool HasChangedKeys => KeyRows.Any(r => r.IsChanged);

    partial void OnKeyFilterChanged(string value) => LoadKeyRows();

    /// <summary>
    /// Arriving at the Keyboard page re-reads the keymap, and leaving any page stops listening.
    /// </summary>
    /// <remarks>
    /// The user's own commands are bindable, and one made a moment ago on the Context menu page is
    /// still waiting on that page's debounce — so it is written first, or it would be missing from
    /// the list it was just created to appear in.
    /// </remarks>
    partial void OnSelectedCategoryChanged(SettingsCategoryViewModel value)
    {
        CancelKeyCapture();
        if (_keymap is null || value.Id != SettingsCategory.Keyboard) return;

        Flush();
        _keymap.Refresh();
        LoadKeyRows();
    }

    partial void OnIsRecordingKeyChanged(bool value) => RecorderStateChanged();

    partial void OnIsFindingKeyChanged(bool value) => RecorderStateChanged();

    partial void OnCanReassignKeyChanged(bool value) => RecorderStateChanged();

    private void RecorderStateChanged()
    {
        OnPropertyChanged(nameof(IsCapturingKey));
        OnPropertyChanged(nameof(ShowsRecorder));
    }

    /// <summary>
    /// Rebuilds the list from the keymap in force, keeping whichever command was selected.
    /// </summary>
    public void LoadKeyRows()
    {
        if (_keymap is null) return;

        var selected = SelectedKeyRow?.Id;
        var keymap = _keymap.Current;
        var bindables = _keymap.Bindables().ToDictionary(b => b.Id, StringComparer.Ordinal);
        var filter = KeyFilter.Trim();

        KeyRows.Clear();

        var rows = CommandCatalog.All
            .Select(c => (c.Id, c.Name, c.Category, c.Icon, c.Aliases))
            .Concat(_settings.CustomCommands.DistinctBy(c => c.Id).Select(c => (
                Id: KeymapService.CustomId(c.Id), c.Name, Category: "Custom commands",
                Icon: (string?)(c.RunElevated ? "Icon.Shield" : "Icon.CustomCommand"),
                Aliases: (IReadOnlyList<string>)[])));

        string? heading = null;
        foreach (var (id, name, category, icon, aliases) in rows)
        {
            if (!bindables.TryGetValue(id, out var bindable)) continue;

            var chords = keymap.ChordsFor(id);
            if (filter.Length > 0 &&
                !CommandMatcher.Match(filter, name, aliases, category, string.Join(" ", chords)).IsMatch &&
                // Any of its shortcuts, not just the first: a command found by its second chord
                // is still the command that key runs.
                !chords.Any(c => c.ToString().Contains(filter, StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }

            if (heading != category)
            {
                KeyRows.Add(KeyBindingRowViewModel.Heading(category));
                heading = category;
            }

            KeyRows.Add(KeyBindingRowViewModel.For(
                id, name, category, icon, bindable.Context, chords, bindable.Defaults,
                changed: !chords.SequenceEqual(bindable.Defaults)));
        }

        SelectedKeyRow = KeyRows.FirstOrDefault(r => r.Id == selected && !r.IsHeading);
        OnPropertyChanged(nameof(HasChangedKeys));
        ResetAllKeysCommand.NotifyCanExecuteChanged();
    }

    /// <summary>Selects one command, clearing the filter if it would hide it. How the command
    /// palette's "change shortcut" lands here.</summary>
    public void SelectKeyCommand(string id)
    {
        if (KeyRows.All(r => r.Id != id)) KeyFilter = "";
        SelectedKeyRow = KeyRows.FirstOrDefault(r => r.Id == id);
    }

    private bool HasKeySelection => _keymap is not null && SelectedKeyRow is { IsHeading: false };

    /// <summary>Starts listening for a shortcut to replace the selected command's.</summary>
    [RelayCommand(CanExecute = nameof(HasKeySelection))]
    public void ChangeKey() => BeginRecording(adds: false);

    /// <summary>Starts listening for a shortcut to add to the ones it has.</summary>
    [RelayCommand(CanExecute = nameof(HasKeySelection))]
    private void AddKey() => BeginRecording(adds: true);

    private void BeginRecording(bool adds)
    {
        if (SelectedKeyRow is not { IsHeading: false } row) return;

        _recordingAdds = adds;
        _pendingChord = null;
        CanReassignKey = false;
        IsFindingKey = false;
        IsRecordingKey = true;
        RecorderText = adds
            ? $"Press another shortcut for “{row.Name}”. Esc cancels."
            : $"Press the new shortcut for “{row.Name}”. Esc cancels.";
    }

    /// <summary>Starts listening for a key to look up: "what does this do?"</summary>
    [RelayCommand]
    private void FindKey()
    {
        if (_keymap is null) return;

        IsRecordingKey = false;
        CanReassignKey = false;
        IsFindingKey = true;
        RecorderText = "Press a shortcut to see what it runs. Esc cancels.";
    }

    [RelayCommand]
    public void CancelKeyCapture()
    {
        IsRecordingKey = false;
        IsFindingKey = false;
        CanReassignKey = false;
        _pendingChord = null;
        RecorderText = "";
    }

    /// <summary>
    /// A chord pressed while the recorder was listening.
    /// </summary>
    public void OfferChord(KeyChord chord)
    {
        if (_keymap is null) return;

        if (IsFindingKey)
        {
            IsFindingKey = false;
            RecorderText = "";
            // The filter does the looking up: the list narrows to what is on that key, with the
            // command it actually runs selected — or to nothing, which is the answer too.
            var runs = _keymap.Current.OwnerOf(chord);
            KeyFilter = chord.ToString();
            SelectedKeyRow = KeyRows.FirstOrDefault(r => r.Id == runs) ?? KeyRows.FirstOrDefault(r => !r.IsHeading);
            return;
        }

        if (!IsRecordingKey || SelectedKeyRow is not { IsHeading: false } row) return;

        if (KeymapRules.Refuse(chord, row.Context) is { } refusal)
        {
            // Still listening: a refused key is a reason to try another, not to start again.
            RecorderText = $"{chord} can’t be used — {refusal} Press another, or Esc to cancel.";
            CanReassignKey = false;
            _pendingChord = null;
            return;
        }

        var owner = _keymap.Current.OwnerOf(chord);
        if (owner is null || owner == row.Id)
        {
            Assign(row, chord);
            return;
        }

        // Somebody else's: say whose, and take it only when asked to. Listening stops here, so
        // that Tab, Space and Enter are keys again and can reach the Reassign button — while it
        // went on, each of them was heard as another chord and withdrew the offer.
        _pendingChord = chord;
        IsRecordingKey = false;
        CanReassignKey = true;
        RecorderText = $"{chord} already runs “{NameOf(owner)}”. Reassign it to “{row.Name}”?";
    }

    /// <summary>Takes the pending chord from the command that has it.</summary>
    [RelayCommand(CanExecute = nameof(CanReassignKey))]
    private void ReassignKey()
    {
        if (_pendingChord is { } chord && SelectedKeyRow is { IsHeading: false } row) Assign(row, chord);
    }

    private void Assign(KeyBindingRowViewModel row, KeyChord chord)
    {
        var current = _keymap!.Current.ChordsFor(row.Id);
        IReadOnlyList<KeyChord> chords = _recordingAdds && !current.Contains(chord) ? [.. current, chord] : [chord];
        if (_recordingAdds && current.Contains(chord)) chords = current;

        CancelKeyCapture();
        _keymap.SetChords(row.Id, chords);
        LoadKeyRows();
    }

    /// <summary>Leaves the selected command with no shortcut at all.</summary>
    [RelayCommand(CanExecute = nameof(CanRemoveKeys))]
    private void RemoveKeys()
    {
        if (SelectedKeyRow is not { IsHeading: false } row) return;

        CancelKeyCapture();
        _keymap!.SetChords(row.Id, []);
        LoadKeyRows();
    }

    private bool CanRemoveKeys => HasKeySelection && SelectedKeyRow!.HasChords;

    /// <summary>Puts the selected command back to the shortcuts it ships with.</summary>
    [RelayCommand(CanExecute = nameof(CanResetKey))]
    private void ResetKey()
    {
        if (SelectedKeyRow is not { IsHeading: false } row) return;

        CancelKeyCapture();
        _keymap!.Reset(row.Id);
        LoadKeyRows();
    }

    private bool CanResetKey => HasKeySelection && SelectedKeyRow!.IsChanged;

    [RelayCommand(CanExecute = nameof(HasChangedKeys))]
    private void ResetAllKeys()
    {
        CancelKeyCapture();
        _keymap!.ResetAll();
        LoadKeyRows();
    }

    private string NameOf(string id) =>
        CommandCatalog.Find(id)?.Name
        ?? _settings.CustomCommands.FirstOrDefault(c => KeymapService.CustomId(c.Id) == id)?.Name
        ?? id;
}
