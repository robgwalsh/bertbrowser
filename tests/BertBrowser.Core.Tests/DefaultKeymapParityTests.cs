using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The shortcuts as they behaved when they were thirty <c>KeyBinding</c>s and two hand-written key
/// handlers, pinned chord by chord against what has focus. The dispatcher that replaced them sees a
/// key before the focused control rather than after, so every protection the old arrangement got
/// for free — Backspace not navigating mid-word, Ctrl+Z still undoing typing — is a rule now, and
/// this table is what says the rules add up to the same thing.
/// </summary>
public class DefaultKeymapParityTests
{
    private static readonly Keymap Keymap = KeymapRules.Resolve(KeymapRules.CatalogBindables(), null);

    private static string? Press(string chord, FocusKind focus, bool settings = false) =>
        KeymapRules.Dispatch(Keymap, KeyChord.Parse(chord), new FocusState(focus, SettingsOpen: settings));

    /// <summary>Bound on the window, with a chord no text box uses: live wherever focus is.</summary>
    [Theory]
    [InlineData("Alt+Left", "nav.back")]
    [InlineData("Alt+Right", "nav.forward")]
    [InlineData("Alt+Up", "nav.up")]
    [InlineData("F5", "nav.refresh")]
    [InlineData("Ctrl+T", "tab.new")]
    [InlineData("Ctrl+W", "tab.close")]
    [InlineData("Ctrl+Tab", "tab.next")]
    [InlineData("Ctrl+Shift+Tab", "tab.previous")]
    [InlineData("Ctrl+1", "tab.goto-1")]
    [InlineData("Ctrl+8", "tab.goto-8")]
    [InlineData("Ctrl+9", "tab.goto-last")]
    [InlineData("Ctrl+Alt+Right", "pane.split-right")]
    [InlineData("Ctrl+Alt+Down", "pane.split-below")]
    [InlineData("Ctrl+Shift+W", "pane.close")]
    [InlineData("F6", "pane.next")]
    [InlineData("Shift+F6", "pane.previous")]
    [InlineData("Ctrl+Shift+D", "tools.disk-usage")]
    [InlineData("Ctrl+Shift+U", "tools.duplicates")]
    [InlineData("Ctrl+Shift+H", "tools.changes")]
    [InlineData("F7", "tools.compare")]
    [InlineData("Ctrl+Shift+T", "tab.reopen")]
    [InlineData("Ctrl+P", "view.preview")]
    [InlineData("Alt+P", "view.preview")]
    [InlineData("Ctrl+M", "view.metadata")]
    [InlineData("Alt+M", "view.metadata")]
    [InlineData("Ctrl+B", "view.flat")]
    [InlineData("Ctrl+F", "search.folder")]
    [InlineData("Ctrl+E", "search.folder")]
    [InlineData("Ctrl+Shift+F", "search.pc")]
    public void Window_shortcuts_fire_wherever_focus_is(string chord, string command)
    {
        Assert.All(Enum.GetValues<FocusKind>(), focus => Assert.Equal(command, Press(chord, focus)));
    }

    /// <summary>Backspace was a window binding too, but a text box handled the key first.</summary>
    [Fact]
    public void Backspace_navigates_except_while_typing()
    {
        Assert.Equal("nav.back", Press("Backspace", FocusKind.FileList));
        Assert.Equal("nav.back", Press("Backspace", FocusKind.Other));
        Assert.Equal("nav.back", Press("Backspace", FocusKind.TextReadOnly));
        Assert.Null(Press("Backspace", FocusKind.TextEditable));
    }

    /// <summary>Guarded by hand with "focus is not a TextBoxBase" — which the preview's read-only
    /// text is too.</summary>
    [Theory]
    [InlineData("Ctrl+Z", "edit.undo")]
    [InlineData("Ctrl+Y", "edit.redo")]
    [InlineData("Ctrl+Shift+Z", "edit.redo")]
    [InlineData("Ctrl+Alt+Z", "edit.undo-history")]
    public void Undo_and_redo_stay_out_of_every_text_control(string chord, string command)
    {
        Assert.Equal(command, Press(chord, FocusKind.FileList));
        Assert.Equal(command, Press(chord, FocusKind.Other));
        Assert.Null(Press(chord, FocusKind.TextEditable));
        Assert.Null(Press(chord, FocusKind.TextReadOnly));
    }

    /// <summary>Handled in the tab's own view, and only while its list had the keyboard.</summary>
    [Theory]
    [InlineData("Enter", "file.open")]
    [InlineData("Ctrl+Shift+Enter", "file.run-as-admin")]
    [InlineData("F2", "file.rename")]
    [InlineData("Del", "file.delete")]
    [InlineData("Shift+Del", "file.delete-permanently")]
    [InlineData("Ctrl+Shift+N", "file.new-folder")]
    [InlineData("Alt+Enter", "file.properties")]
    [InlineData("Ctrl+C", "edit.copy")]
    [InlineData("Ctrl+X", "edit.cut")]
    [InlineData("Ctrl+V", "edit.paste")]
    [InlineData("Ctrl+Shift+C", "edit.copy-path")]
    public void List_shortcuts_belong_to_the_list(string chord, string command)
    {
        Assert.Equal(command, Press(chord, FocusKind.FileList));
        Assert.Null(Press(chord, FocusKind.Other));
        Assert.Null(Press(chord, FocusKind.TextEditable));
        Assert.Null(Press(chord, FocusKind.TextReadOnly));
    }

    /// <summary>The settings page cleared the window's bindings and both handlers returned early.</summary>
    [Theory]
    [InlineData("Backspace")]
    [InlineData("F5")]
    [InlineData("Ctrl+T")]
    [InlineData("Ctrl+W")]
    [InlineData("Ctrl+Z")]
    [InlineData("Ctrl+F")]
    [InlineData("Ctrl+Shift+F")]
    [InlineData("Alt+P")]
    [InlineData("F7")]
    [InlineData("Del")]
    public void Nothing_about_the_folders_fires_while_settings_is_open(string chord)
    {
        Assert.All(Enum.GetValues<FocusKind>(), focus => Assert.Null(Press(chord, focus, settings: true)));
    }
}
