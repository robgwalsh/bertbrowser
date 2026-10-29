using BertBrowser.Core.Services.CommandBars;
using Xunit;

namespace BertBrowser.Core.Tests;

public class CommandBarEditTests
{
    private static BarItem C(string id) => BarItem.Command(id);

    /// <summary>File[ a, Sub[ b, c ], d ] · Edit[ e ] — title bar: t1 t2 — one row: r1.</summary>
    private static readonly CommandBarSet Set = new(
        1,
        [
            BarItem.Menu("&File", C("a"), BarItem.Menu("Sub", C("b"), C("c")), C("d")),
            BarItem.Menu("&Edit", C("e")),
        ],
        new ToolbarSpec([C("t1"), C("t2")]),
        [new ToolbarSpec([C("r1")])]);

    private static string Ids(IEnumerable<BarItem> items) =>
        string.Join(" ", items.Select(i => i.IsSeparator ? "|" : i.Id ?? AccessKeys.Strip(i.Caption ?? "?")));

    [Fact]
    public void A_place_is_a_bar_and_a_path_down_through_its_submenus()
    {
        Assert.Equal("File Edit", Ids(CommandBarEdit.ItemsAt(Set, BarTarget.MenuBar, [])));
        Assert.Equal("a Sub d", Ids(CommandBarEdit.ItemsAt(Set, BarTarget.MenuBar, [0])));
        Assert.Equal("b c", Ids(CommandBarEdit.ItemsAt(Set, BarTarget.MenuBar, [0, 1])));
        Assert.Equal("t1 t2", Ids(CommandBarEdit.ItemsAt(Set, BarTarget.TitleBar, [])));
        Assert.Equal("r1", Ids(CommandBarEdit.ItemsAt(Set, BarTarget.ForRow(0), [])));

        Assert.Equal(["Menu bar", "File", "Sub"], CommandBarEdit.Trail(Set, BarTarget.MenuBar, [0, 1]));
        Assert.Equal(["Toolbar row 1"], CommandBarEdit.Trail(Set, BarTarget.ForRow(0), []));
    }

    [Fact]
    public void A_place_that_is_not_there_is_empty_and_an_edit_aimed_at_it_changes_nothing()
    {
        // Through a command, past the end, and a row that does not exist.
        Assert.Empty(CommandBarEdit.ItemsAt(Set, BarTarget.MenuBar, [0, 0]));
        Assert.False(CommandBarEdit.Exists(Set, BarTarget.MenuBar, [7]));
        Assert.False(CommandBarEdit.Exists(Set, BarTarget.ForRow(3), []));

        Assert.Same(Set, CommandBarEdit.Insert(Set, BarTarget.MenuBar, [0, 0], 0, C("x")));
        Assert.Same(Set, CommandBarEdit.Insert(Set, BarTarget.ForRow(3), [], 0, C("x")));
        Assert.Same(Set, CommandBarEdit.Remove(Set, BarTarget.TitleBar, [], 9));
        Assert.Same(Set, CommandBarEdit.Move(Set, BarTarget.TitleBar, [], 9, 0));
    }

    [Fact]
    public void An_insert_lands_where_asked_and_leaves_every_other_bar_alone()
    {
        var edited = CommandBarEdit.Insert(Set, BarTarget.MenuBar, [0, 1], 1, BarItem.Separator);

        Assert.Equal("b | c", Ids(CommandBarEdit.ItemsAt(edited, BarTarget.MenuBar, [0, 1])));
        Assert.Equal("a Sub d", Ids(CommandBarEdit.ItemsAt(edited, BarTarget.MenuBar, [0])));
        Assert.Same(Set.Menus[1], edited.Menus[1]);
        Assert.Same(Set.TitleBar, edited.TitleBar);
        // Past the end is the end.
        Assert.Equal("t1 t2 x", Ids(CommandBarEdit.ItemsAt(
            CommandBarEdit.Insert(Set, BarTarget.TitleBar, [], 99, C("x")), BarTarget.TitleBar, [])));
    }

    [Fact]
    public void The_menu_bar_itself_takes_menus_only()
    {
        Assert.Same(Set, CommandBarEdit.Insert(Set, BarTarget.MenuBar, [], 0, C("x")));
        Assert.Same(Set, CommandBarEdit.Insert(Set, BarTarget.MenuBar, [], 0, BarItem.Separator));

        var edited = CommandBarEdit.Insert(Set, BarTarget.MenuBar, [], 1, BarItem.Menu("View"));
        Assert.Equal("File View Edit", Ids(edited.Menus));
        // A toolbar takes anything, a submenu included.
        Assert.Equal("Drop t1 t2", Ids(
            CommandBarEdit.Insert(Set, BarTarget.TitleBar, [], 0, BarItem.Menu("Drop")).TitleBar.Items));
    }

    [Fact]
    public void A_submenu_cannot_be_added_past_the_depth_limit()
    {
        var set = Set;
        var path = new List<int>();
        for (var depth = 1; depth <= CommandBarRules.MaxDepth; depth++)
        {
            set = CommandBarEdit.Insert(set, BarTarget.TitleBar, path, 0, BarItem.Menu("m" + depth));
            path.Add(0);
        }

        Assert.Same(set, CommandBarEdit.Insert(set, BarTarget.TitleBar, path, 0, BarItem.Menu("too deep")));
        Assert.NotSame(set, CommandBarEdit.Insert(set, BarTarget.TitleBar, path, 0, C("fine")));
        Assert.True(CommandBarRules.Same(set, CommandBarRules.Clean(set)));
    }

    [Fact]
    public void Move_remove_and_replace_act_on_one_item()
    {
        Assert.Equal("d a Sub", Ids(CommandBarEdit.ItemsAt(
            CommandBarEdit.Move(Set, BarTarget.MenuBar, [0], 2, 0), BarTarget.MenuBar, [0])));
        Assert.Equal("Sub d a", Ids(CommandBarEdit.ItemsAt(
            CommandBarEdit.Move(Set, BarTarget.MenuBar, [0], 0, 2), BarTarget.MenuBar, [0])));
        Assert.Equal("a d", Ids(CommandBarEdit.ItemsAt(
            CommandBarEdit.Remove(Set, BarTarget.MenuBar, [0], 1), BarTarget.MenuBar, [0])));

        var renamed = CommandBarEdit.Replace(Set, BarTarget.MenuBar, [], 1, Set.Menus[1] with { Caption = "E&dit" });
        Assert.Equal("E&dit", renamed.Menus[1].Caption);
        Assert.Equal("e", Ids(renamed.Menus[1].Items!));
    }

    [Fact]
    public void Rows_are_added_empty_and_removed_whole()
    {
        var added = CommandBarEdit.AddRow(Set);

        Assert.Equal(2, added.Rows.Count);
        Assert.Empty(added.Rows[1].Items);
        Assert.Equal("r1", Ids(CommandBarEdit.RemoveRow(added, 1).Rows.Single().Items));
        Assert.Empty(CommandBarEdit.RemoveRow(Set, 0).Rows);
        Assert.Same(Set, CommandBarEdit.RemoveRow(Set, 5));
    }

    [Fact]
    public void Labels_are_a_toolbar_s_own_setting()
    {
        var labelled = CommandBarEdit.WithShowText(Set, BarTarget.ForRow(0), true);

        Assert.True(CommandBarEdit.ShowsText(labelled, BarTarget.ForRow(0)));
        Assert.False(CommandBarEdit.ShowsText(labelled, BarTarget.TitleBar));
        Assert.Same(Set, CommandBarEdit.WithShowText(Set, BarTarget.MenuBar, true));
    }
}
