using System.Text.RegularExpressions;
using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class CommandCatalogTests
{
    [Fact]
    public void Ids_are_unique_dotted_and_lowercase()
    {
        var ids = CommandCatalog.All.Select(c => c.Id).ToList();

        Assert.Equal(ids.Count, ids.Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.All(ids, id => Assert.Matches(new Regex("^[a-z]+\\.[a-z0-9-]+$"), id));
    }

    [Fact]
    public void Every_command_has_a_name_and_a_known_category()
    {
        Assert.All(CommandCatalog.All, c =>
        {
            Assert.False(string.IsNullOrWhiteSpace(c.Name));
            Assert.Contains(c.Category, CommandCategories.All);
        });
    }

    [Fact]
    public void Names_are_unique_within_a_category()
    {
        // Two rows reading the same in the palette cannot be told apart by the person choosing.
        var repeats = CommandCatalog.All
            .GroupBy(c => (c.Category, c.Name))
            .Where(g => g.Count() > 1)
            .Select(g => g.Key.Name);

        Assert.Empty(repeats);
    }

    [Fact]
    public void Icons_are_named_resources()
    {
        Assert.All(CommandCatalog.All.Where(c => c.Icon is not null), c => Assert.StartsWith("Icon.", c.Icon));
    }

    [Fact]
    public void No_two_commands_ship_with_the_same_shortcut()
    {
        var shared = CommandCatalog.All
            .SelectMany(c => c.DefaultChords.Select(chord => (chord, c.Id)))
            .GroupBy(x => x.chord)
            .Where(g => g.Count() > 1)
            .Select(g => $"{g.Key}: {string.Join(", ", g.Select(x => x.Id))}");

        Assert.Empty(shared);
    }

    [Fact]
    public void No_shipped_shortcut_is_one_the_rules_would_refuse()
    {
        var refused = CommandCatalog.All
            .SelectMany(c => c.DefaultChords.Select(chord => (c.Id, chord, Why: KeymapRules.Refuse(chord, c.Context))))
            .Where(x => x.Why is not null)
            .Select(x => $"{x.Id} {x.chord}: {x.Why}");

        Assert.Empty(refused);
    }

    [Fact]
    public void Every_shipped_shortcut_is_in_force_with_nothing_overridden()
    {
        var keymap = KeymapRules.Resolve(KeymapRules.CatalogBindables(), null);

        Assert.All(CommandCatalog.All, c => Assert.Equal(c.DefaultChords, keymap.ChordsFor(c.Id)));
    }

    [Fact]
    public void Found_by_id_exactly()
    {
        Assert.Equal("New tab", CommandCatalog.Find("tab.new")?.Name);
        Assert.Null(CommandCatalog.Find("TAB.NEW"));
        Assert.False(CommandCatalog.IsKnown("no.such"));
    }
}
