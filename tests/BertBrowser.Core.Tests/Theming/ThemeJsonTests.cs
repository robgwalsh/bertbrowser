using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

public class ThemeJsonTests
{
    [Fact]
    public void A_definition_round_trips()
    {
        var original = new ThemeDefinition
        {
            Id = "ocean",
            Name = "Ocean",
            BaseThemeId = "dark-plus",
            IsDark = true,
            Colors = new Dictionary<string, string>(StringComparer.Ordinal)
            {
                [ThemeToken.WindowBackground] = "#0B1E2D",
                [ThemeToken.AccentBackground] = "#1F6FEB",
            },
        };

        Assert.True(ThemeJson.TryDeserialize(ThemeJson.Serialize(original), out var round, out var error));
        Assert.Null(error);
        Assert.NotNull(round);
        Assert.Equal(original.Id, round!.Id);
        Assert.Equal(original.Name, round.Name);
        Assert.Equal(original.BaseThemeId, round.BaseThemeId);
        Assert.Equal(original.IsDark, round.IsDark);
        Assert.Equal(original.Colors, round.Colors);
    }

    [Fact]
    public void A_hand_written_file_loads()
    {
        const string json = """
            {
              // a user could reasonably write this by hand
              "id": "ocean",
              "name": "Ocean",
              "base": "dark-plus",
              "isDark": true,
              "colors": {
                "Theme.Window.Background": "#0B1E2D",
              }
            }
            """;

        Assert.True(ThemeJson.TryDeserialize(json, out var theme, out _));
        Assert.Equal("ocean", theme!.Id);
        Assert.Equal("dark-plus", theme.BaseThemeId);
        Assert.Equal("#0B1E2D", theme.Colors[ThemeToken.WindowBackground]);
    }

    [Fact]
    public void Properties_a_future_version_adds_are_ignored()
    {
        const string json = """
            { "id": "ocean", "name": "Ocean", "somethingNew": 42, "colors": {} }
            """;

        Assert.True(ThemeJson.TryDeserialize(json, out var theme, out _));
        Assert.Equal("ocean", theme!.Id);
    }

    [Fact]
    public void A_theme_with_no_name_falls_back_to_its_id()
    {
        Assert.True(ThemeJson.TryDeserialize("""{ "id": "ocean" }""", out var theme, out _));
        Assert.Equal("ocean", theme!.Name);
    }

    [Theory]
    [InlineData("")]
    [InlineData("not json")]
    [InlineData("{ \"name\": \"No id\" }")]
    [InlineData("null")]
    public void Malformed_input_is_rejected_without_throwing(string json)
    {
        Assert.False(ThemeJson.TryDeserialize(json, out var theme, out var error));
        Assert.Null(theme);
        Assert.False(string.IsNullOrWhiteSpace(error));
    }

    /// <summary>
    /// An exported theme has to stand on its own: no base, every token spelled out, and re-importing
    /// it must give back exactly the colours that were on screen.
    /// </summary>
    [Fact]
    public void An_exported_theme_is_self_contained()
    {
        var monokai = ThemeResolver.Resolve(ThemeCatalog.Monokai, ThemeCatalog.Find);

        Assert.True(ThemeJson.TryDeserialize(ThemeJson.SerializeResolved(monokai), out var exported, out _));
        Assert.Null(exported!.BaseThemeId);
        Assert.Equal(ThemeToken.All.Count, exported.Colors.Count);

        // Resolved without any lookup at all — nothing to inherit from but the built-in floor.
        var reimported = ThemeResolver.Resolve(exported);
        foreach (var key in ThemeToken.All)
        {
            Assert.Equal(monokai[key], reimported[key]);
        }
    }
}
