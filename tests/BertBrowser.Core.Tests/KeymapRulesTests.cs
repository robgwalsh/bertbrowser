using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class KeymapRulesTests
{
    private static readonly Bindable[] Three =
    [
        new("a.one", CommandContext.Browser, [KeyChord.Parse("Ctrl+1")]),
        new("b.two", CommandContext.Browser, [KeyChord.Parse("Ctrl+2"), KeyChord.Parse("F2")]),
        new("c.three", CommandContext.FileList, []),
    ];

    private static Dictionary<string, IReadOnlyList<string>> Overrides(
        params (string Id, string[] Chords)[] entries) =>
        entries.ToDictionary(e => e.Id, e => (IReadOnlyList<string>)e.Chords);

    private static string Chords(Keymap keymap, string id) => string.Join(" ", keymap.ChordsFor(id));

    // --- Resolve ---

    [Fact]
    public void Nothing_overridden_is_the_defaults()
    {
        var keymap = KeymapRules.Resolve(Three, null);

        Assert.Equal("Ctrl+1", Chords(keymap, "a.one"));
        Assert.Equal("Ctrl+2 F2", Chords(keymap, "b.two"));
        Assert.Equal("", Chords(keymap, "c.three"));
        Assert.Equal("b.two", keymap.OwnerOf(KeyChord.Parse("F2")));
        Assert.Equal("Ctrl+2", keymap.GestureText("b.two"));
        Assert.Equal("", keymap.GestureText("c.three"));
    }

    [Fact]
    public void An_override_replaces_a_commands_chords_outright()
    {
        var keymap = KeymapRules.Resolve(Three, Overrides(("b.two", ["Ctrl+K"])));

        Assert.Equal("Ctrl+K", Chords(keymap, "b.two"));
        Assert.Null(keymap.OwnerOf(KeyChord.Parse("F2")));
    }

    [Fact]
    public void An_empty_override_takes_the_default_away()
    {
        var keymap = KeymapRules.Resolve(Three, Overrides(("a.one", [])));

        Assert.Equal("", Chords(keymap, "a.one"));
        Assert.Null(keymap.OwnerOf(KeyChord.Parse("Ctrl+1")));
    }

    [Fact]
    public void The_users_choice_beats_another_commands_default()
    {
        // c.three comes after a.one in the catalogue, and still wins: it was chosen, the other was
        // merely shipped.
        var keymap = KeymapRules.Resolve(Three, Overrides(("c.three", ["Ctrl+1"])));

        Assert.Equal("c.three", keymap.OwnerOf(KeyChord.Parse("Ctrl+1")));
        Assert.Equal("", Chords(keymap, "a.one"));
    }

    [Fact]
    public void Two_overrides_claiming_one_chord_go_to_the_earlier_command()
    {
        var keymap = KeymapRules.Resolve(Three, Overrides(("b.two", ["Ctrl+K"]), ("a.one", ["Ctrl+K"])));

        Assert.Equal("a.one", keymap.OwnerOf(KeyChord.Parse("Ctrl+K")));
        Assert.Equal("", Chords(keymap, "b.two"));
    }

    [Fact]
    public void A_saved_chord_that_is_unreadable_or_refused_is_dropped_not_fatal()
    {
        var keymap = KeymapRules.Resolve(Three, Overrides(("a.one", ["Ctrl+Banana", "Q", "Ctrl+Q"])));

        Assert.Equal("Ctrl+Q", Chords(keymap, "a.one"));
    }

    [Fact]
    public void An_override_for_a_command_nobody_knows_is_ignored()
    {
        var keymap = KeymapRules.Resolve(Three, Overrides(("from.elsewhere", ["Ctrl+1"])));

        Assert.Equal("a.one", keymap.OwnerOf(KeyChord.Parse("Ctrl+1")));
        Assert.Null(keymap.ContextOf("from.elsewhere"));
    }

    // --- WithChords / WithDefaults ---

    [Fact]
    public void Giving_a_chord_takes_it_from_whoever_had_it()
    {
        var overrides = KeymapRules.WithChords(Three, null, "c.three", [KeyChord.Parse("F2")]);
        var keymap = KeymapRules.Resolve(Three, overrides);

        Assert.Equal("F2", Chords(keymap, "c.three"));
        Assert.Equal("Ctrl+2", Chords(keymap, "b.two"));
        Assert.Equal(["Ctrl+2"], overrides["b.two"]);
    }

    [Fact]
    public void Setting_a_command_back_to_what_it_ships_with_stores_nothing()
    {
        var changed = KeymapRules.WithChords(Three, null, "a.one", [KeyChord.Parse("Ctrl+K")]);
        var back = KeymapRules.WithChords(Three, changed, "a.one", [KeyChord.Parse("Ctrl+1")]);

        Assert.Empty(back);
    }

    [Fact]
    public void An_unknown_override_survives_an_edit_to_something_else()
    {
        var before = Overrides(("custom:gone", ["Ctrl+G"]));

        var after = KeymapRules.WithChords(Three, before, "a.one", []);

        Assert.Equal(["Ctrl+G"], after["custom:gone"]);
        Assert.Empty(after["a.one"]);
    }

    [Fact]
    public void Reset_removes_the_override()
    {
        var changed = KeymapRules.WithChords(Three, null, "a.one", []);

        Assert.Empty(KeymapRules.WithDefaults(Three, changed, "a.one"));
        Assert.Empty(KeymapRules.WithDefaults(Three, null, "a.one"));
    }

    [Fact]
    public void Reset_takes_a_default_back_from_the_command_it_was_given_to()
    {
        // c.three was given b.two's F2. Resetting b.two has to bring F2 home, or b.two would read
        // "as shipped" while having lost a key.
        var given = KeymapRules.WithChords(Three, null, "c.three", [KeyChord.Parse("F2")]);

        var reset = KeymapRules.Resolve(Three, KeymapRules.WithDefaults(Three, given, "b.two"));

        Assert.Equal("Ctrl+2 F2", Chords(reset, "b.two"));
        Assert.Equal("", Chords(reset, "c.three"));
    }

    [Fact]
    public void Two_commands_sharing_an_id_cost_one_its_shortcut_not_the_app_its_startup()
    {
        Bindable[] doubled =
        [
            new("custom:x", CommandContext.Browser, [KeyChord.Parse("Ctrl+1")]),
            new("custom:x", CommandContext.Browser, [KeyChord.Parse("Ctrl+2")]),
        ];

        Assert.Equal("Ctrl+1", Chords(KeymapRules.Resolve(doubled, null), "custom:x"));
    }

    [Theory]
    [InlineData("tab.next", true)]
    [InlineData("view.thumbs-larger", true)]
    [InlineData("tab.close", false)]
    [InlineData("pane.copy-to-other", false)]
    [InlineData("app.palette", false)]
    [InlineData("app.exit", false)]
    [InlineData("edit.undo", false)]
    public void Only_stepping_commands_repeat_while_the_key_is_held(string id, bool repeats)
    {
        Assert.Equal(repeats, KeymapRules.RepeatsWhenHeld(id));
    }

    // --- Refuse ---

    [Theory]
    [InlineData("P", CommandContext.Browser)]
    [InlineData("Shift+P", CommandContext.FileList)]
    [InlineData("Space", CommandContext.FileList)]
    [InlineData("Down", CommandContext.FileList)]
    [InlineData("Shift+Home", CommandContext.Browser)]
    [InlineData("Tab", CommandContext.FileList)]
    [InlineData("Esc", CommandContext.App)]
    [InlineData("Enter", CommandContext.Browser)]
    [InlineData("Del", CommandContext.App)]
    [InlineData("Num+", CommandContext.Browser)]
    [InlineData("Alt+F4", CommandContext.App)]
    [InlineData("Alt+Space", CommandContext.Browser)]
    public void Keys_that_already_mean_something_cannot_be_bound(string chord, CommandContext context)
    {
        Assert.NotNull(KeymapRules.Refuse(KeyChord.Parse(chord), context));
    }

    [Theory]
    [InlineData("F5", CommandContext.Browser)]
    [InlineData("Shift+F6", CommandContext.Browser)]
    [InlineData("Backspace", CommandContext.Browser)]
    [InlineData("Enter", CommandContext.FileList)]
    [InlineData("Shift+Del", CommandContext.FileList)]
    [InlineData("Num+", CommandContext.FileList)]
    [InlineData("Ctrl+P", CommandContext.Browser)]
    [InlineData("Alt+Left", CommandContext.Browser)]
    [InlineData("Ctrl+Alt+Down", CommandContext.Browser)]
    [InlineData("Ctrl+,", CommandContext.App)]
    public void Everything_else_can(string chord, CommandContext context)
    {
        Assert.Null(KeymapRules.Refuse(KeyChord.Parse(chord), context));
    }

    // --- Typing ---

    [Theory]
    [InlineData("Backspace")]
    [InlineData("Enter")]
    [InlineData("Shift+Del")]
    [InlineData("Ctrl+A")]
    [InlineData("Ctrl+C")]
    [InlineData("Ctrl+V")]
    [InlineData("Ctrl+Z")]
    [InlineData("Ctrl+Y")]
    [InlineData("Ctrl+Shift+Z")]
    [InlineData("Ctrl+Left")]
    [InlineData("Ctrl+Shift+End")]
    [InlineData("Ctrl+Backspace")]
    [InlineData("Ctrl+Alt+Z")]
    [InlineData("Ctrl+Alt+Shift+2")]
    public void A_text_box_keeps_the_keys_it_uses(string chord)
    {
        Assert.False(KeymapRules.FiresInEditableText(KeyChord.Parse(chord)));
    }

    [Theory]
    [InlineData("F5")]
    [InlineData("Shift+F6")]
    [InlineData("Ctrl+T")]
    [InlineData("Ctrl+Shift+P")]
    [InlineData("Ctrl+Tab")]
    [InlineData("Alt+Left")]
    [InlineData("Alt+P")]
    [InlineData("Ctrl+Alt+Right")]
    [InlineData("Ctrl+F")]
    public void And_gives_up_the_ones_it_does_not(string chord)
    {
        Assert.True(KeymapRules.FiresInEditableText(KeyChord.Parse(chord)));
    }

    [Theory]
    [InlineData("Backspace", true)]
    [InlineData("F5", true)]
    [InlineData("Ctrl+T", true)]
    [InlineData("Alt+Left", true)]
    [InlineData("Ctrl+C", false)]
    [InlineData("Ctrl+A", false)]
    [InlineData("Ctrl+Z", false)]
    [InlineData("Ctrl+Alt+Z", false)]
    [InlineData("Down", false)]
    [InlineData("Ctrl+Home", false)]
    public void Read_only_text_keeps_selection_copying_and_the_caret(string chord, bool fires)
    {
        Assert.Equal(fires, KeymapRules.FiresInReadOnlyText(KeyChord.Parse(chord)));
    }

    // --- Dispatch ---

    private static readonly Bindable[] Contexts =
    [
        new("app.cmd", CommandContext.App, [KeyChord.Parse("Ctrl+Shift+P")]),
        new("browser.cmd", CommandContext.Browser, [KeyChord.Parse("Ctrl+T")]),
        new("list.cmd", CommandContext.FileList, [KeyChord.Parse("Del")]),
    ];

    private static string? Press(string chord, FocusState state) =>
        KeymapRules.Dispatch(KeymapRules.Resolve(Contexts, null), KeyChord.Parse(chord), state);

    [Fact]
    public void An_unbound_chord_is_nobodys()
    {
        Assert.Null(Press("Ctrl+K", new FocusState(FocusKind.Other)));
    }

    [Fact]
    public void A_list_command_needs_the_list_to_have_the_keyboard()
    {
        Assert.Equal("list.cmd", Press("Del", new FocusState(FocusKind.FileList)));
        Assert.Null(Press("Del", new FocusState(FocusKind.Other)));
        Assert.Null(Press("Del", new FocusState(FocusKind.TextEditable)));
    }

    [Fact]
    public void The_settings_page_silences_everything_about_the_folders()
    {
        var settings = new FocusState(FocusKind.Other, SettingsOpen: true);

        Assert.Null(Press("Ctrl+T", settings));
        Assert.Null(Press("Del", new FocusState(FocusKind.FileList, SettingsOpen: true)));
        Assert.Equal("app.cmd", Press("Ctrl+Shift+P", settings));
    }

    [Fact]
    public void The_open_palette_leaves_only_what_works_from_anywhere()
    {
        var palette = new FocusState(FocusKind.TextEditable, PaletteOpen: true);

        Assert.Null(Press("Ctrl+T", palette));
        Assert.Equal("app.cmd", Press("Ctrl+Shift+P", palette));
    }

    [Fact]
    public void Nothing_fires_behind_a_dialog_or_into_the_recorder()
    {
        Assert.Null(Press("Ctrl+T", new FocusState(FocusKind.Other, ContentBlocked: true)));
        Assert.Null(Press("Ctrl+Shift+P", new FocusState(FocusKind.Other, Recording: true)));
    }

    [Fact]
    public void A_menu_keeps_its_own_keys()
    {
        // A context menu opened on the file list: focus is "in the list" as far as the tree is
        // concerned, and Del or Enter must still be the menu's.
        Assert.Null(Press("Del", new FocusState(FocusKind.FileList, InPopup: true)));
        Assert.Null(Press("Ctrl+T", new FocusState(FocusKind.Other, InPopup: true)));
        Assert.Null(Press("Ctrl+Shift+P", new FocusState(FocusKind.Other, InPopup: true)));
    }
}
