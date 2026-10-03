namespace BertBrowser.Core.Services.Commands;

/// <summary>Something a shortcut can be bound to: a catalogue command, or one of the user's own.</summary>
public readonly record struct Bindable(string Id, CommandContext Context, IReadOnlyList<KeyChord> Defaults);

/// <summary>What has the keyboard, as far as a shortcut needs to know.</summary>
public enum FocusKind
{
    /// <summary>A file list.</summary>
    FileList,

    /// <summary>A box somebody is typing in — search, path, a settings field.</summary>
    TextEditable,

    /// <summary>Selectable text nobody can type in: the preview pane's.</summary>
    TextReadOnly,

    /// <summary>Anything else: the folder tree, a tab, a button.</summary>
    Other,
}

/// <summary>The state of the window at the moment a key arrives.</summary>
/// <param name="ContentBlocked">A modal dialog is up. The window still receives keys behind it,
/// because a themed window keeps its owner's handle enabled so it can be dragged.</param>
/// <param name="Recording">The Keyboard page is waiting for a chord, and must get it.</param>
/// <param name="InPopup">
/// A menu, a dropdown or some other popup has the keyboard. Its keys still travel through the
/// window on the way to it — a popup routes its events up through whatever it was opened from —
/// and the dispatcher sees them first, so without this Enter on a menu item would open the
/// selected file instead of choosing the item.
/// </param>
public readonly record struct FocusState(
    FocusKind Focus,
    bool SettingsOpen = false,
    bool PaletteOpen = false,
    bool ContentBlocked = false,
    bool Recording = false,
    bool InPopup = false);

/// <summary>
/// The shortcuts in force: the shipped defaults with the user's changes over them. Each chord
/// belongs to at most one command, so pressing it has one meaning wherever it is live.
/// </summary>
public sealed class Keymap
{
    private readonly Dictionary<string, IReadOnlyList<KeyChord>> _chords;
    private readonly Dictionary<KeyChord, string> _owners;
    private readonly Dictionary<string, CommandContext> _contexts;

    internal Keymap(
        Dictionary<string, IReadOnlyList<KeyChord>> chords,
        Dictionary<KeyChord, string> owners,
        Dictionary<string, CommandContext> contexts)
    {
        _chords = chords;
        _owners = owners;
        _contexts = contexts;
    }

    /// <summary>The chords that run this command, the one menus print first.</summary>
    public IReadOnlyList<KeyChord> ChordsFor(string id) => _chords.GetValueOrDefault(id) ?? [];

    /// <summary>The command a chord belongs to, wherever it happens to be live.</summary>
    public string? OwnerOf(KeyChord chord) => _owners.GetValueOrDefault(chord);

    public CommandContext? ContextOf(string id) =>
        _contexts.TryGetValue(id, out var context) ? context : null;

    /// <summary>What a menu or tooltip prints beside the command: its first chord, or nothing.</summary>
    public string GestureText(string id) => ChordsFor(id) is [var first, ..] ? first.ToString() : "";
}

/// <summary>
/// Everything about shortcuts that can be decided without a keyboard: which chords are in force,
/// which may be bound at all, and — the part that used to be three hand-written key handlers —
/// whether a chord fires given what has focus.
/// </summary>
/// <remarks>
/// <para>
/// The dispatcher sees a key <em>before</em> the focused control does. The window-level
/// <c>KeyBinding</c>s this replaced saw it after, which is what quietly kept Backspace from
/// navigating while somebody typed in the search box: the box handled the key and the binding never
/// heard of it. That protection is now a rule here (<see cref="FiresInEditableText"/>), and it is a
/// rule about the <em>chord</em> rather than the command, because once chords can be rebound nobody
/// can say in advance which command will be sitting on Backspace.
/// </para>
/// <para>
/// A custom command's id is kept in the overrides whether or not anything by that id exists right
/// now, and so is an id from another build: dropping a binding because its owner is momentarily
/// unknown would lose it for good on the next save.
/// </para>
/// </remarks>
public static class KeymapRules
{
    /// <summary>The catalogue's commands as the keymap sees them.</summary>
    public static IEnumerable<Bindable> CatalogBindables() =>
        CommandCatalog.All.Select(c => new Bindable(c.Id, c.Context, c.DefaultChords));

    /// <summary>
    /// The keymap in force. An override replaces a command's chords outright — an empty list means
    /// "no shortcut", which is how a default is taken away. Where two commands end up claiming one
    /// chord, the user's choice beats a default and an earlier command beats a later one; the loser
    /// simply does not have it, rather than the key doing two things.
    /// </summary>
    public static Keymap Resolve(
        IEnumerable<Bindable> bindables,
        IReadOnlyDictionary<string, IReadOnlyList<string>>? overrides)
    {
        // One entry per id, the first: two of the user's commands sharing an id is a hand-edited
        // settings file, and it must cost one of them its shortcut rather than the app its startup.
        var all = bindables.DistinctBy(b => b.Id, StringComparer.Ordinal).ToList();
        var chords = new Dictionary<string, IReadOnlyList<KeyChord>>(StringComparer.Ordinal);
        var owners = new Dictionary<KeyChord, string>();
        var contexts = all.ToDictionary(b => b.Id, b => b.Context, StringComparer.Ordinal);
        var claimed = new Dictionary<string, List<KeyChord>>(StringComparer.Ordinal);

        // Two passes, so a chord the user gave to one command is taken before another command's
        // default can claim it — whichever of the two comes first in the catalogue.
        foreach (var overridden in new[] { true, false })
        {
            foreach (var bindable in all)
            {
                var saved = overrides is not null && overrides.TryGetValue(bindable.Id, out var texts) ? texts : null;
                if ((saved is not null) != overridden) continue;

                var wanted = saved is null
                    ? bindable.Defaults
                    : [.. saved.Select(t => KeyChord.TryParse(t, out var c) ? c : (KeyChord?)null)
                        .OfType<KeyChord>()
                        .Where(c => Refuse(c, bindable.Context) is null)];

                var mine = new List<KeyChord>();
                foreach (var chord in wanted)
                    if (owners.TryAdd(chord, bindable.Id))
                        mine.Add(chord);
                claimed[bindable.Id] = mine;
            }
        }

        foreach (var (id, mine) in claimed) chords[id] = mine;
        return new Keymap(chords, owners, contexts);
    }

    /// <summary>
    /// Why this chord may not be bound to a command of this context, or null when it may. These are
    /// the keys that already mean something to whatever has focus, with nothing the dispatcher
    /// could check to tell the two meanings apart.
    /// </summary>
    public static string? Refuse(KeyChord chord, CommandContext context)
    {
        var bare = !chord.Has(ChordModifiers.Ctrl) && !chord.Has(ChordModifiers.Alt);

        if (chord is { Modifiers: ChordModifiers.Alt, Key: "F4" })
            return "Alt+F4 closes the window.";
        if (chord.Has(ChordModifiers.Alt) && !chord.Has(ChordModifiers.Ctrl) && chord.Key == "Space")
            return "Alt+Space opens the window menu.";

        if (!bare) return null;

        switch (chord.Kind)
        {
            case ChordKeyKind.Function:
                return null;

            // A letter on its own is type-ahead in a file list and typing everywhere else.
            case ChordKeyKind.Printable:
                return "A key that types a character needs Ctrl or Alt with it.";

            // The number pad's keys type too, but a file list has no use for "+" as type-ahead,
            // and Num+ / Num- selecting by pattern is what every commander does with them.
            case ChordKeyKind.Numpad:
                return context == CommandContext.FileList
                    ? null
                    : "A number-pad key on its own can only be given to a command that acts on the file list.";

            case ChordKeyKind.Caret:
                return "The arrow and paging keys move through lists, so they need Ctrl or Alt with them.";

            default:
                // Enter, Backspace, Del and Insert are free in a file list. Tab and Esc are not
                // free anywhere: one moves focus and the other backs out of whatever is open.
                if (chord.Key is "Tab" or "Esc")
                    return $"{chord.Key} is used to move around the window.";
                if (chord.Key == "Backspace") return null;
                return context == CommandContext.FileList
                    ? null
                    : $"{chord.Key} on its own can only be given to a command that acts on the file list.";
        }
    }

    /// <summary>
    /// Whether a chord may fire while somebody is typing. Anything the text box would use itself
    /// does not: plain and shifted keys, the clipboard and undo chords, word-wise caret movement,
    /// and Ctrl+Alt with a printable key — which is AltGr, and types a character on many layouts.
    /// </summary>
    public static bool FiresInEditableText(KeyChord chord)
    {
        var ctrl = chord.Has(ChordModifiers.Ctrl);
        var alt = chord.Has(ChordModifiers.Alt);

        if (!ctrl && !alt) return chord.Kind == ChordKeyKind.Function;
        if (IsTextCommand(chord)) return false;
        if (ctrl && alt) return chord.Kind is not (ChordKeyKind.Printable or ChordKeyKind.Numpad);

        // Ctrl+Left, Ctrl+Shift+End, Ctrl+Backspace, Ctrl+Insert: the box's own.
        if (ctrl && !alt && (chord.Kind == ChordKeyKind.Caret || chord.Key is "Backspace" or "Del" or "Insert"))
            return false;

        return true;
    }

    /// <summary>
    /// Whether a chord may fire over selectable read-only text. Nothing can be typed there, so far
    /// more is allowed than in a box — but selecting, copying and moving the caret still belong to
    /// the text, and undo stays off as it always was over the preview.
    /// </summary>
    public static bool FiresInReadOnlyText(KeyChord chord)
    {
        var ctrl = chord.Has(ChordModifiers.Ctrl);
        var alt = chord.Has(ChordModifiers.Alt);

        if (IsTextCommand(chord)) return false;
        if (ctrl && alt) return chord.Kind is not (ChordKeyKind.Printable or ChordKeyKind.Numpad);
        if (!alt && chord.Kind == ChordKeyKind.Caret) return false;
        return true;
    }

    /// <summary>
    /// The command this key press runs, or null when it is nobody's here and must carry on to the
    /// focused control. Says nothing about whether the command is <em>available</em> — a claimed
    /// chord is the command's even when it can do nothing, so that it never falls through to
    /// type-ahead.
    /// </summary>
    public static string? Dispatch(Keymap keymap, KeyChord chord, FocusState state)
    {
        if (state.ContentBlocked || state.Recording || state.InPopup) return null;
        if (keymap.OwnerOf(chord) is not { } id || keymap.ContextOf(id) is not { } context) return null;

        // While the palette is up its own box has the keyboard, and the only shortcuts that still
        // mean anything are the ones that would work from anywhere — which is how the chord that
        // opened it also closes it.
        if ((state.PaletteOpen || state.SettingsOpen) && context != CommandContext.App) return null;
        if (context == CommandContext.FileList && state.Focus != FocusKind.FileList) return null;

        return state.Focus switch
        {
            FocusKind.TextEditable when !FiresInEditableText(chord) => null,
            FocusKind.TextReadOnly when !FiresInReadOnlyText(chord) => null,
            _ => id,
        };
    }

    /// <summary>
    /// The overrides after giving <paramref name="id"/> exactly these chords, taking each away from
    /// whichever command held it. An entry that ends up equal to its defaults is dropped, so the
    /// stored map stays "what the user changed" and a later build's new default still arrives.
    /// </summary>
    public static IReadOnlyDictionary<string, IReadOnlyList<string>> WithChords(
        IEnumerable<Bindable> bindables,
        IReadOnlyDictionary<string, IReadOnlyList<string>>? overrides,
        string id,
        IReadOnlyList<KeyChord> chords)
    {
        var all = bindables.ToList();
        var keymap = Resolve(all, overrides);
        var result = overrides is null
            ? new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal)
            : new Dictionary<string, IReadOnlyList<string>>(overrides, StringComparer.Ordinal);

        foreach (var chord in chords)
        {
            if (keymap.OwnerOf(chord) is not { } owner || owner == id) continue;
            Store(owner, [.. keymap.ChordsFor(owner).Where(c => c != chord)]);
            keymap = Resolve(all, result);
        }

        Store(id, [.. chords.Distinct()]);
        return result;

        void Store(string target, IReadOnlyList<KeyChord> wanted)
        {
            var defaults = all.FirstOrDefault(b => b.Id == target).Defaults ?? [];
            if (wanted.SequenceEqual(defaults)) result.Remove(target);
            else result[target] = [.. wanted.Select(c => c.ToString())];
        }
    }

    /// <summary>
    /// The overrides after putting this command back to the shortcuts it ships with — taking each
    /// back from any command it had been given to. Without that, resetting a command whose default
    /// now belongs to another would leave it "as shipped" and with no key at all.
    /// </summary>
    public static IReadOnlyDictionary<string, IReadOnlyList<string>> WithDefaults(
        IEnumerable<Bindable> bindables,
        IReadOnlyDictionary<string, IReadOnlyList<string>>? overrides,
        string id)
    {
        var all = bindables.ToList();
        return WithChords(all, overrides, id, all.FirstOrDefault(b => b.Id == id).Defaults ?? []);
    }

    /// <summary>
    /// Whether holding a command's key down should run it again and again. Almost nothing should:
    /// a held Ctrl+W closing every tab, or a held copy queueing the same files ten times, is what
    /// auto-repeat does to a command that was only ever meant once. These few are steps, where
    /// holding the key is how you take several.
    /// </summary>
    public static bool RepeatsWhenHeld(string id) => id is
        "tab.next" or "tab.previous" or "tab.move-left" or "tab.move-right" or
        "pane.next" or "pane.previous" or "view.thumbs-larger" or "view.thumbs-smaller";

    /// <summary>Ctrl+A/C/X/V/Z/Y and Ctrl+Shift+Z: select, clipboard, undo — a text control's own.</summary>
    private static bool IsTextCommand(KeyChord chord) =>
        (chord.Modifiers == ChordModifiers.Ctrl && chord.Key is "A" or "C" or "X" or "V" or "Z" or "Y") ||
        (chord.Modifiers == (ChordModifiers.Ctrl | ChordModifiers.Shift) && chord.Key == "Z") ||
        (chord.Modifiers == ChordModifiers.Shift && chord.Key is "Del" or "Insert");
}
