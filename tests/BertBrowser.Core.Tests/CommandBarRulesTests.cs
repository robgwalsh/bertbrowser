using System.Text.Json;
using BertBrowser.Core.Services.CommandBars;
using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class CommandBarRulesTests
{
    /// <summary>A source that knows the catalogue, with whatever the test says is unavailable,
    /// ticked or listed.</summary>
    private sealed class Source : IBarSource
    {
        public Dictionary<string, string> Unavailable { get; } = [];
        public HashSet<string> Checked { get; } = [];
        public HashSet<string> Lists { get; } = [];
        public Dictionary<string, string> Live { get; } = [];

        public BarCommandState? Command(string id) =>
            CommandCatalog.Find(id) is { } c
                ? new BarCommandState(c.Name, c.Icon, "", Unavailable.GetValueOrDefault(id), Checked.Contains(id),
                    Live.GetValueOrDefault(id))
                : null;

        public bool ListHasRows(string listId) => Lists.Contains(listId);
    }

    private static string Show(IEnumerable<BarRow> rows) =>
        string.Join(" ", rows.Select(r => r.Kind switch
        {
            BarRowKind.Separator => "|",
            BarRowKind.Menu => $"{AccessKeys.Strip(r.Caption)}[{Show(r.Children)}]",
            BarRowKind.List => "@" + r.Id,
            BarRowKind.Placeholder => $"({r.Caption})",
            _ => r.Id,
        }));

    private static BarItem C(string id, string? caption = null) => BarItem.Command(id, caption);

    // --- The default ---

    [Fact]
    public void Every_command_and_list_in_the_default_exists()
    {
        var items = CommandBarRules.Flatten(CommandBarDefaults.Default).ToList();

        Assert.All(items.Where(i => i.IsCommand), i => Assert.True(CommandCatalog.IsKnown(i.Id!), i.Id));
        Assert.All(items.Where(i => i.IsList), i => Assert.NotNull(CommandBarLists.Find(i.Id)));
    }

    [Fact]
    public void The_default_has_no_separator_doubled_or_at_an_edge()
    {
        var menus = CommandBarRules.Flatten(CommandBarDefaults.Default).Where(i => i.IsMenu).Select(i => i.Items!)
            .Append(CommandBarDefaults.Default.TitleBar.Items);

        Assert.All(menus, items =>
        {
            Assert.False(items[0].IsSeparator);
            Assert.False(items[^1].IsSeparator);
            Assert.DoesNotContain(items.Zip(items.Skip(1)), pair => pair.First.IsSeparator && pair.Second.IsSeparator);
        });
    }

    [Fact]
    public void Top_level_access_keys_are_distinct_and_clear_of_shipped_alt_chords()
    {
        var keys = CommandBarDefaults.Default.Menus.Select(m => AccessKeys.KeyOf(m.Caption!)).ToList();

        Assert.DoesNotContain(null, keys);
        Assert.Equal(keys.Count, keys.Distinct().Count());

        // Alt+D, Alt+P, Alt+M today — and whatever is added later.
        var claimed = CommandCatalog.All.SelectMany(c => c.DefaultChords).Select(c => c.ToString())
            .Where(c => c.StartsWith("Alt+", StringComparison.Ordinal) && c.Length == 5)
            .Select(c => (char?)c[4]);
        Assert.Empty(keys.Intersect(claimed));
    }

    [Fact]
    public void The_default_toolbar_is_the_buttons_the_title_bar_always_had()
    {
        Assert.Equal(
            "edit.undo edit.redo | tools.disk-usage tools.duplicates tools.changes tools.compare app.settings",
            Show(CommandBarRules.Present(CommandBarDefaults.Default.TitleBar.Items, new Source(), accessKeys: false)));
        Assert.Empty(CommandBarDefaults.Default.Rows);
    }

    [Fact]
    public void The_default_is_still_the_default_after_a_trip_through_the_settings_file()
    {
        var json = JsonSerializer.Serialize(CommandBarDefaults.Default);
        var back = CommandBarRules.Clean(JsonSerializer.Deserialize<CommandBarSet>(json));

        Assert.True(CommandBarRules.IsDefault(back));
        Assert.DoesNotContain("null", json);
    }

    [Fact]
    public void Every_short_caption_belongs_to_a_command()
    {
        Assert.All(CommandBarCaptions.Ids, id => Assert.True(CommandCatalog.IsKnown(id), id));
    }

    // --- Clean ---

    [Fact]
    public void Nothing_stored_means_the_default()
    {
        Assert.Same(CommandBarDefaults.Default, CommandBarRules.Clean(null));
    }

    [Fact]
    public void A_file_with_holes_in_it_is_made_walkable()
    {
        var holed = JsonSerializer.Deserialize<CommandBarSet>(
            """{"Menus":[{"Kind":"menu","Caption":" &File ","Items":[{"Kind":"command"},null,{"Kind":"command","Id":" tab.new "},{}]},{"Kind":"menu"},{"Kind":"command","Id":"tab.new"}]}""");

        var clean = CommandBarRules.Clean(holed);

        var file = Assert.Single(clean.Menus);
        Assert.Equal("&File", file.Caption);
        Assert.Equal("tab.new", Assert.Single(file.Items!).Id);
        Assert.Empty(clean.TitleBar.Items);
        Assert.Empty(clean.Rows);
    }

    [Fact]
    public void A_command_or_kind_this_build_does_not_know_is_kept()
    {
        var set = new CommandBarSet(9, [BarItem.Menu("Later", C("from.a-later-build"), new BarItem("gauge", "disk"))],
            new ToolbarSpec([]), []);

        var clean = CommandBarRules.Clean(set);

        Assert.True(CommandBarRules.Same(set, clean));
        Assert.Equal(9, clean.Revision);
    }

    [Fact]
    public void Menus_stop_nesting_at_the_limit()
    {
        var deep = BarItem.Menu("1", BarItem.Menu("2", BarItem.Menu("3", BarItem.Menu("4", BarItem.Menu("5", C("tab.new"))))));

        var clean = CommandBarRules.Clean(new CommandBarSet(1, [deep], new ToolbarSpec([]), []));

        Assert.Equal(["1", "2", "3", "4"], CommandBarRules.Flatten(clean.Menus).Select(i => i.Caption));
    }

    // --- Same / Unplaced ---

    [Fact]
    public void A_moved_item_a_new_caption_or_a_label_switch_is_a_different_arrangement()
    {
        var d = CommandBarDefaults.Default;

        Assert.True(CommandBarRules.IsDefault(d with { Revision = 99 }));
        Assert.False(CommandBarRules.IsDefault(d with { Menus = [.. d.Menus.Reverse()] }));
        Assert.False(CommandBarRules.IsDefault(d with { TitleBar = d.TitleBar with { ShowText = true } }));
        Assert.False(CommandBarRules.IsDefault(d with { Rows = [new ToolbarSpec([])] }));
        Assert.False(CommandBarRules.IsDefault(
            d with { Menus = [d.Menus[0] with { Caption = "F&ile" }, .. d.Menus.Skip(1)] }));
    }

    [Fact]
    public void Unplaced_lists_what_is_on_no_menu_and_no_toolbar()
    {
        var set = new CommandBarSet(1, [BarItem.Menu("A", BarItem.Menu("B", C("tab.new")))],
            new ToolbarSpec([C("tab.close")]), [new ToolbarSpec([C("nav.up")])]);

        Assert.Equal(["nav.back", "custom:x"],
            CommandBarRules.Unplaced(set, ["tab.new", "nav.back", "tab.close", "nav.up", "custom:x"]));
    }

    [Fact]
    public void The_default_leaves_out_only_what_the_palette_is_the_place_for()
    {
        var unplaced = CommandBarRules.Unplaced(CommandBarDefaults.Default, CommandCatalog.All.Select(c => c.Id));

        Assert.All(unplaced, id => Assert.True(
            id.StartsWith("tab.goto-", StringComparison.Ordinal) || id.StartsWith("tools.indexer-", StringComparison.Ordinal) ||
            id is "view.side-pane" or "view.toggle-thumbnails" or "search.stop" or "edit.clear-undo-history"
                or "app.match-windows-theme",
            id));
    }

    // --- Present ---

    [Fact]
    public void An_unavailable_command_stays_and_says_why()
    {
        var source = new Source { Unavailable = { ["nav.back"] = "There is nowhere to go back to." } };

        var row = Assert.Single(CommandBarRules.Present([C("nav.back")], source));

        Assert.False(row.IsEnabled);
        Assert.Equal("There is nowhere to go back to.", row.Reason);
    }

    [Fact]
    public void What_nothing_answers_to_goes_and_takes_its_separators_with_it()
    {
        var rows = CommandBarRules.Present(
            [BarItem.Separator, C("tab.new"), BarItem.Separator, C("custom:deleted"), BarItem.Separator,
             BarItem.List(CommandBarLists.Drives), BarItem.Separator, C("tab.close"), BarItem.Separator],
            new Source());

        Assert.Equal("tab.new | tab.close", Show(rows));
    }

    [Fact]
    public void An_empty_list_leaves_its_line_or_nothing_and_an_empty_submenu_goes()
    {
        var items = new[]
        {
            BarItem.Menu("Workspaces", BarItem.List(CommandBarLists.Workspaces)),
            BarItem.Menu("Your commands", BarItem.List(CommandBarLists.CustomCommands)),
            BarItem.Menu("Nothing", BarItem.Separator),
        };

        Assert.Equal("Workspaces[(No saved workspaces)]", Show(CommandBarRules.Present(items, new Source())));
        Assert.Equal(
            "Workspaces[@workspaces] Your commands[@custom-commands]",
            Show(CommandBarRules.Present(items,
                new Source { Lists = { CommandBarLists.Workspaces, CommandBarLists.CustomCommands } })));
    }

    [Fact]
    public void A_caption_is_the_spot_s_own_then_the_moment_s_then_the_menu_s_then_the_name()
    {
        var source = new Source { Live = { ["file.delete"] = "Delete 3 items…" } };

        var rows = CommandBarRules.Present(
            [C("view.sort-name", "Name"), C("file.delete"), C("view.preview"), C("tab.new")], source, accessKeys: false);

        Assert.Equal(["Name", "Delete 3 items…", "Preview pane", "New tab"], rows.Select(r => r.Caption));
        Assert.Equal("Toggle preview pane", rows[2].Name);
    }

    [Fact]
    public void An_empty_caption_for_the_moment_is_no_caption()
    {
        // What Undo describes when there is nothing to undo.
        var source = new Source { Live = { ["edit.undo"] = "" } };

        Assert.Equal("Undo", Assert.Single(CommandBarRules.Present([C("edit.undo")], source, accessKeys: false)).Caption);
    }

    [Fact]
    public void A_menu_gives_each_row_a_key_and_a_toolbar_gives_none()
    {
        BarItem[] items = [C("edit.cut"), C("edit.copy"), C("edit.copy-path"), BarItem.Menu("&Select", C("select.all"))];
        var source = new Source { Checked = { "edit.cut" } };

        var menu = CommandBarRules.Present(items, source);
        var toolbar = CommandBarRules.Present(items, source, accessKeys: false, showText: true);

        Assert.Equal(["&Cut", "C&opy", "Copy &as path", "&Select"], menu.Select(r => r.Caption));
        Assert.Equal(["Cut", "Copy", "Copy as path", "Select"], toolbar.Select(r => r.Caption));
        Assert.True(menu[0].IsChecked);
        Assert.All(toolbar, r => Assert.True(r.ShowText));
        // Under a toolbar's dropdown the rows are menu rows again.
        Assert.Equal("&Select all", toolbar[3].Children[0].Caption);
    }

    [Fact]
    public void A_name_from_data_cannot_mark_a_key()
    {
        var source = new Source { Live = { ["tab.new"] = "R&D" } };

        var row = Assert.Single(CommandBarRules.Present([C("tab.new")], source));

        Assert.Equal("&R&&D", row.Caption);
        Assert.Equal("_R&D", AccessKeys.ToUnderscored(row.Caption));
    }

    // --- Access keys ---

    [Fact]
    public void A_marked_key_is_kept_and_the_rest_take_what_is_free()
    {
        Assert.Equal(
            ["&Date modified", "D&ate created", "Da&te", "&Name", "&&"],
            AccessKeys.Assign(["Date modified", "D&ate created", "Date", "&Name", "&&"]));
        // Every letter gone: it goes without rather than share.
        Assert.Equal(["&A", "A"], AccessKeys.Assign(["A", "A"]));
    }

    [Fact]
    public void Underscores_survive_the_translation_to_wpf()
    {
        Assert.Equal("_File", AccessKeys.ToUnderscored("&File"));
        Assert.Equal("my__file _x", AccessKeys.ToUnderscored("my_file &x"));
        Assert.Equal("Cut & paste", AccessKeys.ToUnderscored("Cut && paste"));
        Assert.Equal("Cut & paste", AccessKeys.Strip("Cut && paste"));
    }

    // --- Toolbars ---

    [Fact]
    public void Undo_and_redo_carry_their_lists()
    {
        Assert.Equal(CommandBarLists.UndoSteps, CommandBarRules.DropdownFor("edit.undo"));
        Assert.Equal(CommandBarLists.RedoSteps, CommandBarRules.DropdownFor("edit.redo"));
        Assert.Null(CommandBarRules.DropdownFor("edit.cut"));
    }

    [Fact]
    public void A_toolbar_shows_what_fits_and_leaves_room_for_the_overflow_button()
    {
        double[] widths = [30, 30, 30, 30];

        Assert.Equal(4, CommandBarRules.Fit(widths, 120, overflowWidth: 20));
        Assert.Equal(3, CommandBarRules.Fit(widths, 119, overflowWidth: 20));
        Assert.Equal(2, CommandBarRules.Fit(widths, 100, overflowWidth: 20));
        Assert.Equal(0, CommandBarRules.Fit(widths, 10, overflowWidth: 20));
        Assert.Equal(0, CommandBarRules.Fit([], 0, overflowWidth: 20));
    }
}
