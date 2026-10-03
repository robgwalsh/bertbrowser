namespace BertBrowser.Core.Services.Commands;

[Flags]
public enum ChordModifiers
{
    None = 0,
    Ctrl = 1,
    Alt = 2,
    Shift = 4,
}

/// <summary>What a key does when nothing is bound to it, which is what decides where a chord built
/// on it may fire.</summary>
public enum ChordKeyKind
{
    /// <summary>Types a character: letters, digits, punctuation, Space.</summary>
    Printable,

    /// <summary>F1 to F24 — never typed, never navigation.</summary>
    Function,

    /// <summary>Enter, Backspace, Del, Insert, Tab, Esc.</summary>
    Editing,

    /// <summary>The arrows, Home, End, PageUp, PageDown.</summary>
    Caret,

    /// <summary>The number pad's digits and operators.</summary>
    Numpad,
}

/// <summary>
/// One keyboard shortcut — modifiers and a key — as the keymap stores it and the menus print it.
/// Core has no WPF, so the key is a name of its own (<c>P</c>, <c>F7</c>, <c>Del</c>, <c>Num+</c>);
/// the App converts a <c>KeyEventArgs</c> to one of these and nothing here ever sees a virtual key.
/// </summary>
/// <remarks>
/// The text form is the persisted form: <c>Ctrl+Shift+P</c>, modifiers in that fixed order, which is
/// the order every gesture string in the app already used. A value is only ever made by
/// <see cref="TryParse"/> or <see cref="Create"/>, so two equal chords compare equal.
/// </remarks>
public readonly record struct KeyChord
{
    private static readonly Dictionary<string, (string Name, ChordKeyKind Kind)> Keys = BuildKeys();

    private KeyChord(ChordModifiers modifiers, string key, ChordKeyKind kind)
    {
        Modifiers = modifiers;
        Key = key;
        Kind = kind;
    }

    public ChordModifiers Modifiers { get; }

    /// <summary>The key's canonical name.</summary>
    public string Key { get; }

    public ChordKeyKind Kind { get; }

    public bool Has(ChordModifiers modifier) => (Modifiers & modifier) != 0;

    /// <summary>Whether <paramref name="key"/> names a key a chord can be built on.</summary>
    public static bool IsKey(string key) => Keys.ContainsKey(key);

    /// <summary>A chord from its parts, or null when the key is not one a chord can use.</summary>
    public static KeyChord? Create(ChordModifiers modifiers, string key) =>
        Keys.TryGetValue(key, out var known) ? new KeyChord(modifiers, known.Name, known.Kind) : null;

    public static bool TryParse(string? text, out KeyChord chord)
    {
        chord = default;
        var rest = text?.Trim() ?? "";
        var modifiers = ChordModifiers.None;

        // Modifiers come off the front, and only while something is left after them — so "Ctrl+="
        // and "Num+" both read as a key rather than as a dangling separator.
        while (true)
        {
            if (Strip(ref rest, "Ctrl+") || Strip(ref rest, "Control+")) modifiers |= ChordModifiers.Ctrl;
            else if (Strip(ref rest, "Alt+")) modifiers |= ChordModifiers.Alt;
            else if (Strip(ref rest, "Shift+")) modifiers |= ChordModifiers.Shift;
            else break;
        }

        if (Create(modifiers, rest) is not { } parsed) return false;
        chord = parsed;
        return true;
    }

    public static KeyChord Parse(string text) =>
        TryParse(text, out var chord)
            ? chord
            : throw new FormatException($"'{text}' is not a keyboard shortcut.");

    public override string ToString()
    {
        if (Key is null) return "";
        var text = "";
        if (Has(ChordModifiers.Ctrl)) text += "Ctrl+";
        if (Has(ChordModifiers.Alt)) text += "Alt+";
        if (Has(ChordModifiers.Shift)) text += "Shift+";
        return text + Key;
    }

    private static bool Strip(ref string text, string prefix)
    {
        if (text.Length <= prefix.Length ||
            !text.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) return false;
        text = text[prefix.Length..];
        return true;
    }

    private static Dictionary<string, (string, ChordKeyKind)> BuildKeys()
    {
        var keys = new Dictionary<string, (string, ChordKeyKind)>(StringComparer.OrdinalIgnoreCase);

        void Add(ChordKeyKind kind, string name, params string[] aliases)
        {
            keys[name] = (name, kind);
            foreach (var alias in aliases) keys[alias] = (name, kind);
        }

        for (var c = 'A'; c <= 'Z'; c++) Add(ChordKeyKind.Printable, c.ToString());
        for (var c = '0'; c <= '9'; c++) Add(ChordKeyKind.Printable, c.ToString());
        foreach (var punctuation in new[] { ",", ".", "/", ";", "'", "[", "]", "\\", "-", "=", "`" })
            Add(ChordKeyKind.Printable, punctuation);
        Add(ChordKeyKind.Printable, "Space");

        for (var f = 1; f <= 24; f++) Add(ChordKeyKind.Function, "F" + f);

        Add(ChordKeyKind.Editing, "Enter", "Return");
        Add(ChordKeyKind.Editing, "Backspace", "Back");
        Add(ChordKeyKind.Editing, "Del", "Delete");
        Add(ChordKeyKind.Editing, "Insert", "Ins");
        Add(ChordKeyKind.Editing, "Tab");
        Add(ChordKeyKind.Editing, "Esc", "Escape");

        Add(ChordKeyKind.Caret, "Left");
        Add(ChordKeyKind.Caret, "Right");
        Add(ChordKeyKind.Caret, "Up");
        Add(ChordKeyKind.Caret, "Down");
        Add(ChordKeyKind.Caret, "Home");
        Add(ChordKeyKind.Caret, "End");
        Add(ChordKeyKind.Caret, "PageUp", "PgUp");
        Add(ChordKeyKind.Caret, "PageDown", "PgDn");

        for (var n = 0; n <= 9; n++) Add(ChordKeyKind.Numpad, "Num" + n);
        Add(ChordKeyKind.Numpad, "Num+");
        Add(ChordKeyKind.Numpad, "Num-");
        Add(ChordKeyKind.Numpad, "Num*");
        Add(ChordKeyKind.Numpad, "Num/");
        Add(ChordKeyKind.Numpad, "Num.");

        return keys;
    }
}
