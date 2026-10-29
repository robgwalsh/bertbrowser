using BertBrowser.Core.Services.ShellMenu;
using Xunit;

namespace BertBrowser.Core.Tests;

public class MenuLayoutRulesTests
{
    private static readonly IReadOnlySet<string> AllBuiltIns =
        BuiltInMenuItems.All.Select(i => i.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);

    private static string Show(IEnumerable<MenuPlacement> placements) =>
        string.Join(" ", placements.Select(p => p.Kind switch
        {
            MenuPlacementKind.Separator => "|",
            MenuPlacementKind.Command => "cmd:" + p.Id,
            _ => p.Id,
        }));

    [Fact]
    public void The_default_places_every_built_in_once()
    {
        var placed = MenuLayoutRules.Default
            .Select(t => MenuLayoutRules.IsBuiltIn(t, out var id) ? id : null)
            .OfType<string>()
            .ToList();

        Assert.Equal(BuiltInMenuItems.All.Select(i => i.Id), placed);
        Assert.Single(MenuLayoutRules.Default, MenuLayoutRules.More);
    }

    [Fact]
    public void No_layout_means_the_default()
    {
        Assert.Same(MenuLayoutRules.Default, MenuLayoutRules.Normalize(null, []));
    }

    [Fact]
    public void A_built_in_nobody_placed_goes_back_beside_its_default_neighbour()
    {
        // As a later build adding "copy-name" would look to a layout saved before it existed.
        var layout = MenuLayoutRules.Normalize(
            ["app:properties", "-", "app:copy-path", "app:cut", MenuLayoutRules.More],
            hiddenBuiltIns: BuiltInMenuItems.All.Select(i => i.Id)
                .Where(id => id is not ("copy-name" or "copy-path" or "cut" or "properties")));

        Assert.Equal(
            ["app:properties", "-", "app:copy-path", "app:copy-name", "app:cut", MenuLayoutRules.More],
            layout);
    }

    [Fact]
    public void A_hidden_built_in_is_not_put_back()
    {
        var hidden = BuiltInMenuItems.All.Select(i => i.Id).Where(id => id != "open").ToList();

        var layout = MenuLayoutRules.Normalize([MenuLayoutRules.More], hidden);

        Assert.Equal(["app:open", MenuLayoutRules.More], layout);
    }

    [Fact]
    public void A_missing_catch_all_is_restored_and_a_repeat_dropped()
    {
        var hidden = BuiltInMenuItems.All.Select(i => i.Id).Where(id => id is not ("bookmark" or "properties"));

        var layout = MenuLayoutRules.Normalize(
            ["app:bookmark", "-", "app:BOOKMARK", "-", "app:properties"], hidden);

        Assert.Equal(["app:bookmark", MenuLayoutRules.More, "-", "-", "app:properties"], layout);
    }

    [Fact]
    public void A_token_this_build_does_not_know_is_kept()
    {
        var layout = MenuLayoutRules.Normalize(
            ["app:from-a-later-build", "clsid:{abc}", MenuLayoutRules.More],
            BuiltInMenuItems.All.Select(i => i.Id));

        Assert.Equal(["app:from-a-later-build", "clsid:{abc}", MenuLayoutRules.More], layout);
    }

    [Fact]
    public void The_default_arrangement_is_the_menu_as_it_was()
    {
        var arranged = MenuLayoutRules.Arrange(
            MenuLayoutRules.Default, AllBuiltIns, ["c1"], ["verb:git_shell", "clsid:{7z}"]);

        Assert.EndsWith(
            "rename delete delete-permanently bookmark | cmd:c1 | verb:git_shell clsid:{7z} | properties",
            Show(arranged));
    }

    [Fact]
    public void The_tree_skips_what_it_does_not_have()
    {
        var tree = new HashSet<string>(["new", "open-terminal", "copy-path", "properties"]);

        var arranged = MenuLayoutRules.Arrange(MenuLayoutRules.Default, tree, [], []);

        Assert.Equal("new | open-terminal | copy-path | | | properties", Show(arranged));
    }

    [Fact]
    public void A_placed_extension_appears_at_its_spot_and_not_again_at_the_catch_all()
    {
        var layout = MenuLayoutRules.Normalize(
            ["app:compress", "clsid:{7z}", "-", "app:cut", MenuLayoutRules.More, "cmd:c2"],
            BuiltInMenuItems.All.Select(i => i.Id).Where(id => id is not ("compress" or "cut")));

        var arranged = MenuLayoutRules.Arrange(
            layout, AllBuiltIns, ["c1", "c2"], ["verb:git_shell", "clsid:{7z}"]);

        Assert.Equal("compress clsid:{7z} | cut | cmd:c1 | verb:git_shell cmd:c2", Show(arranged));
    }

    [Fact]
    public void What_is_not_offered_here_is_never_emitted()
    {
        // A command not applicable to this selection, an extension with nothing for it, and a
        // built-in this menu does not declare.
        var arranged = MenuLayoutRules.Arrange(
            ["app:cut", "cmd:c1", "clsid:{7z}", MenuLayoutRules.More], new HashSet<string>(), [], []);

        Assert.Empty(arranged);
    }
}
