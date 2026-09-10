using BertBrowser.Core.Services.ShellMenu;
using Xunit;

namespace BertBrowser.Core.Tests;

public class BuiltInMenuItemsTests
{
    [Fact]
    public void Ids_are_unique_and_lowercase()
    {
        var ids = BuiltInMenuItems.All.Select(i => i.Id).ToList();

        Assert.Equal(ids.Count, ids.Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.All(ids, id => Assert.Equal(id.ToLowerInvariant(), id));
    }

    [Fact]
    public void Every_row_is_in_at_least_one_menu()
    {
        Assert.All(BuiltInMenuItems.All, i =>
            Assert.True(i.IsIn(BuiltInMenuPlaces.FileList) || i.IsIn(BuiltInMenuPlaces.FolderTree)));
    }

    [Fact]
    public void Shown_unless_hidden()
    {
        var hidden = new HashSet<string>(["copy-path"], StringComparer.OrdinalIgnoreCase);

        Assert.False(BuiltInMenuItems.IsShown("copy-path", hidden));
        Assert.False(BuiltInMenuItems.IsShown("Copy-Path", hidden));
        Assert.True(BuiltInMenuItems.IsShown("open", hidden));
    }

    [Fact]
    public void An_unknown_id_is_a_bug_not_a_preference()
    {
        Assert.Throws<ArgumentException>(() =>
            BuiltInMenuItems.IsShown("no-such-item", new HashSet<string>()));
    }

    [Fact]
    public void Hidden_list_survives_a_save_through_the_shared_rule()
    {
        // The same rule the shell extensions use: an id this build does not know (an older or newer
        // version's) stays hidden rather than being dropped on the first save.
        var hidden = ShellMenuRules.HiddenAfterSave(
            ["from-another-version", "open"],
            BuiltInMenuItems.All.Select(i => (i.Id, IsShown: i.Id != "cut")));

        Assert.Equal(["from-another-version", "cut"], hidden);
    }

    [Theory]
    [InlineData(BuiltInMenuPlaces.FileList, "file list")]
    [InlineData(BuiltInMenuPlaces.FolderTree, "folder tree")]
    [InlineData(BuiltInMenuPlaces.Both, "file list and tree")]
    public void Places_are_worded(BuiltInMenuPlaces places, string expected)
    {
        Assert.Equal(expected, BuiltInMenuItems.PlacesText(places));
    }
}

public class MenuSeparatorRulesTests
{
    private static MenuSlot Item(bool visible = true) => new(IsSeparator: false, IsVisible: visible);
    private static MenuSlot Sep() => new(IsSeparator: true, IsVisible: true);

    [Fact]
    public void Separators_between_visible_groups_stay()
    {
        var result = MenuSeparatorRules.Apply([Item(), Sep(), Item(), Sep(), Item()]);

        Assert.Equal([true, true, true, true, true], result);
    }

    [Fact]
    public void A_hidden_group_does_not_leave_two_separators_touching()
    {
        var result = MenuSeparatorRules.Apply([Item(), Sep(), Item(false), Item(false), Sep(), Item()]);

        Assert.Equal([true, true, false, false, false, true], result);
    }

    [Fact]
    public void No_separator_at_either_edge()
    {
        var result = MenuSeparatorRules.Apply([Sep(), Item(false), Sep(), Item(), Sep(), Item(false), Sep()]);

        Assert.Equal([false, false, false, true, false, false, false], result);
    }

    [Fact]
    public void Everything_hidden_shows_nothing()
    {
        var result = MenuSeparatorRules.Apply([Item(false), Sep(), Item(false)]);

        Assert.All(result, Assert.False);
    }

    [Fact]
    public void Adjacent_separators_already_in_the_markup_collapse_to_one()
    {
        // The custom-command and shell-extension anchors sit right beside a plain separator.
        var result = MenuSeparatorRules.Apply([Item(), Sep(), Sep(), Sep(), Item()]);

        Assert.Equal([true, true, false, false, true], result);
    }
}
