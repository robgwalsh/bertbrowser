using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class KeyChordTests
{
    [Theory]
    [InlineData("Ctrl+T", "Ctrl+T")]
    [InlineData("ctrl+shift+p", "Ctrl+Shift+P")]
    [InlineData("Shift+Ctrl+P", "Ctrl+Shift+P")]
    [InlineData("Alt+Ctrl+Z", "Ctrl+Alt+Z")]
    [InlineData(" F7 ", "F7")]
    [InlineData("Delete", "Del")]
    [InlineData("Shift+delete", "Shift+Del")]
    [InlineData("Alt+Return", "Alt+Enter")]
    [InlineData("Back", "Backspace")]
    [InlineData("Ctrl+PgDn", "Ctrl+PageDown")]
    public void Text_round_trips_to_one_spelling(string text, string expected)
    {
        Assert.True(KeyChord.TryParse(text, out var chord));
        Assert.Equal(expected, chord.ToString());
        Assert.Equal(chord, KeyChord.Parse(chord.ToString()));
    }

    [Theory]
    [InlineData("Ctrl+=", ChordModifiers.Ctrl, "=")]
    [InlineData("Ctrl+,", ChordModifiers.Ctrl, ",")]
    [InlineData("Num+", ChordModifiers.None, "Num+")]
    [InlineData("Ctrl+Num+", ChordModifiers.Ctrl, "Num+")]
    [InlineData("Ctrl+Num-", ChordModifiers.Ctrl, "Num-")]
    [InlineData("Ctrl+-", ChordModifiers.Ctrl, "-")]
    public void A_key_that_is_itself_punctuation_is_not_mistaken_for_a_separator(
        string text, ChordModifiers modifiers, string key)
    {
        var chord = KeyChord.Parse(text);

        Assert.Equal(modifiers, chord.Modifiers);
        Assert.Equal(key, chord.Key);
    }

    [Theory]
    [InlineData("")]
    [InlineData("Ctrl+")]
    [InlineData("Ctrl")]
    [InlineData("Win+E")]
    [InlineData("Ctrl+Banana")]
    [InlineData("Ctrl++")]
    public void Nonsense_is_not_a_chord(string text)
    {
        Assert.False(KeyChord.TryParse(text, out _));
        Assert.Throws<FormatException>(() => KeyChord.Parse(text));
    }

    [Theory]
    [InlineData("P", ChordKeyKind.Printable)]
    [InlineData("Space", ChordKeyKind.Printable)]
    [InlineData("F12", ChordKeyKind.Function)]
    [InlineData("Enter", ChordKeyKind.Editing)]
    [InlineData("Home", ChordKeyKind.Caret)]
    [InlineData("Num5", ChordKeyKind.Numpad)]
    public void Each_key_knows_what_it_does_unbound(string key, ChordKeyKind kind)
    {
        Assert.Equal(kind, KeyChord.Parse(key).Kind);
    }

    [Fact]
    public void Equal_chords_are_equal_however_they_were_spelled()
    {
        Assert.Equal(KeyChord.Parse("Ctrl+Shift+Delete"), KeyChord.Parse("shift+ctrl+del"));
        Assert.NotEqual(KeyChord.Parse("Ctrl+P"), KeyChord.Parse("Ctrl+Shift+P"));
    }
}
