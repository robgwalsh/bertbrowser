using System.Windows.Input;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.Services.Commands;

/// <summary>
/// Turns a WPF key event into the <see cref="KeyChord"/> Core's keymap is written in. The one place
/// a virtual key meets a chord name.
/// </summary>
internal static class KeyChordInterop
{
    /// <summary>The chord this key press spells, or null for a press that cannot be one: a
    /// modifier on its own, a key an input method is still composing, or anything with the
    /// Windows key held — those belong to the shell.</summary>
    public static KeyChord? FromEvent(KeyEventArgs e)
    {
        // An Alt chord arrives as Key.System with the real key in SystemKey. ImeProcessed and
        // DeadCharProcessed mean the key is half of a character still being composed, which is
        // the text box's business however the chord would have read.
        var key = e.Key switch
        {
            Key.System => e.SystemKey,
            Key.ImeProcessed or Key.DeadCharProcessed => Key.None,
            _ => e.Key,
        };

        return From(key, Keyboard.Modifiers);
    }

    public static KeyChord? From(Key key, ModifierKeys modifiers)
    {
        if ((modifiers & ModifierKeys.Windows) != 0) return null;
        if (NameOf(key) is not { } name) return null;

        var chord = ChordModifiers.None;
        if ((modifiers & ModifierKeys.Control) != 0) chord |= ChordModifiers.Ctrl;
        if ((modifiers & ModifierKeys.Alt) != 0) chord |= ChordModifiers.Alt;
        if ((modifiers & ModifierKeys.Shift) != 0) chord |= ChordModifiers.Shift;

        return KeyChord.Create(chord, name);
    }

    private static string? NameOf(Key key) => key switch
    {
        >= Key.A and <= Key.Z => key.ToString(),
        >= Key.D0 and <= Key.D9 => ((char)('0' + (key - Key.D0))).ToString(),
        >= Key.NumPad0 and <= Key.NumPad9 => "Num" + (key - Key.NumPad0),
        >= Key.F1 and <= Key.F24 => key.ToString(),

        Key.Add => "Num+",
        Key.Subtract => "Num-",
        Key.Multiply => "Num*",
        Key.Divide => "Num/",
        Key.Decimal => "Num.",

        // The OEM keys by what a US layout prints on them. The position is what is matched, so on
        // another layout the chord is the same key under a different cap.
        Key.OemComma => ",",
        Key.OemPeriod => ".",
        Key.OemQuestion => "/",
        Key.OemSemicolon => ";",
        Key.OemQuotes => "'",
        Key.OemOpenBrackets => "[",
        Key.OemCloseBrackets => "]",
        Key.OemPipe => "\\",
        Key.OemMinus => "-",
        Key.OemPlus => "=",
        Key.OemTilde => "`",

        Key.Space => "Space",
        Key.Enter => "Enter",
        Key.Back => "Backspace",
        Key.Delete => "Del",
        Key.Insert => "Insert",
        Key.Tab => "Tab",
        Key.Escape => "Esc",
        Key.Left => "Left",
        Key.Right => "Right",
        Key.Up => "Up",
        Key.Down => "Down",
        Key.Home => "Home",
        Key.End => "End",
        Key.PageUp => "PageUp",
        Key.PageDown => "PageDown",
        _ => null,
    };
}
