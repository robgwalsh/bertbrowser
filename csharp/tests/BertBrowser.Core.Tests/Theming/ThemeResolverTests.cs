using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

public class ThemeResolverTests
{
    private static ThemeDefinition Theme(
        string id,
        string? baseId = null,
        params (string Token, string Color)[] colors) => new()
    {
        Id = id,
        Name = id,
        BaseThemeId = baseId,
        Colors = colors.ToDictionary(c => c.Token, c => c.Color, StringComparer.Ordinal),
    };

    private static Func<string, ThemeDefinition?> Lookup(params ThemeDefinition[] themes)
    {
        var byId = themes.ToDictionary(t => t.Id, StringComparer.OrdinalIgnoreCase);
        return id => byId.TryGetValue(id, out var theme) ? theme
            : ThemeCatalog.Find(id);
    }

    [Fact]
    public void A_sparse_theme_inherits_everything_it_does_not_set()
    {
        var mine = Theme("mine", "dark-plus", (ThemeToken.AccentBackground, "#C586C0"));

        var resolved = ThemeResolver.Resolve(mine, ThemeCatalog.Find);

        Assert.Equal(ThemeColor.Parse("#C586C0"), resolved[ThemeToken.AccentBackground]);
        Assert.Equal(ThemeColor.Parse("#1E1E1E"), resolved[ThemeToken.WindowBackground]);
        Assert.Empty(resolved.Issues);
    }

    [Fact]
    public void Resolution_is_always_complete()
    {
        var resolved = ThemeResolver.Resolve(Theme("empty"), ThemeCatalog.Find);

        foreach (var key in ThemeToken.All)
        {
            Assert.True(resolved.Colors.ContainsKey(key), $"missing {key}");
        }
    }

    [Fact]
    public void A_theme_beats_its_base_and_a_base_beats_its_own_base()
    {
        var grandparent = Theme("grandparent", null, (ThemeToken.TextPrimary, "#111111"), (ThemeToken.TextMuted, "#999999"));
        var parent = Theme("parent", "grandparent", (ThemeToken.TextPrimary, "#222222"));
        var child = Theme("child", "parent", (ThemeToken.TextPrimary, "#333333"));

        var resolved = ThemeResolver.Resolve(child, Lookup(grandparent, parent, child));

        Assert.Equal(ThemeColor.Parse("#333333"), resolved[ThemeToken.TextPrimary]);
        Assert.Equal(ThemeColor.Parse("#999999"), resolved[ThemeToken.TextMuted]);
    }

    [Fact]
    public void User_overrides_beat_the_theme()
    {
        var mine = Theme("mine", "dark-plus", (ThemeToken.AccentBackground, "#C586C0"));
        var overrides = new Dictionary<string, string> { [ThemeToken.AccentBackground] = "#4EC9B0" };

        var resolved = ThemeResolver.Resolve(mine, ThemeCatalog.Find, overrides);

        Assert.Equal(ThemeColor.Parse("#4EC9B0"), resolved[ThemeToken.AccentBackground]);
    }

    [Fact]
    public void A_missing_base_falls_back_to_the_default_and_says_so()
    {
        var orphan = Theme("orphan", "no-such-theme", (ThemeToken.TextPrimary, "#123456"));

        var resolved = ThemeResolver.Resolve(orphan, ThemeCatalog.Find);

        Assert.Equal(ThemeColor.Parse("#123456"), resolved[ThemeToken.TextPrimary]);
        Assert.Equal(ThemeColor.Parse("#1E1E1E"), resolved[ThemeToken.WindowBackground]);
        Assert.Contains(resolved.Issues, i => i.Message.Contains("no-such-theme", StringComparison.Ordinal));
    }

    [Fact]
    public void A_base_loop_terminates_and_is_reported()
    {
        var a = Theme("a", "b", (ThemeToken.TextPrimary, "#AAAAAA"));
        var b = Theme("b", "a", (ThemeToken.TextMuted, "#BBBBBB"));

        var resolved = ThemeResolver.Resolve(a, Lookup(a, b));

        Assert.Equal(ThemeColor.Parse("#AAAAAA"), resolved[ThemeToken.TextPrimary]);
        Assert.Equal(ThemeColor.Parse("#BBBBBB"), resolved[ThemeToken.TextMuted]);
        Assert.Contains(resolved.Issues, i => i.Severity == ThemeIssueSeverity.Error);
    }

    [Fact]
    public void A_theme_that_bases_on_itself_terminates()
    {
        var self = Theme("self", "self", (ThemeToken.TextPrimary, "#ABCDEF"));

        var resolved = ThemeResolver.Resolve(self, Lookup(self));

        Assert.Equal(ThemeColor.Parse("#ABCDEF"), resolved[ThemeToken.TextPrimary]);
        Assert.Contains(resolved.Issues, i => i.Severity == ThemeIssueSeverity.Error);
    }

    [Fact]
    public void An_unknown_token_is_reported_and_ignored()
    {
        var mine = Theme("mine", "dark-plus", ("Theme.Not.AThing", "#FF0000"));

        var resolved = ThemeResolver.Resolve(mine, ThemeCatalog.Find);

        var issue = Assert.Single(resolved.Issues);
        Assert.Equal("Theme.Not.AThing", issue.Token);
        Assert.Equal(ThemeIssueSeverity.Warning, issue.Severity);
    }

    [Fact]
    public void An_unparseable_colour_keeps_the_inherited_value_and_is_reported()
    {
        var mine = Theme("mine", "dark-plus", (ThemeToken.WindowBackground, "octarine"));

        var resolved = ThemeResolver.Resolve(mine, ThemeCatalog.Find);

        Assert.Equal(ThemeColor.Parse("#1E1E1E"), resolved[ThemeToken.WindowBackground]);
        Assert.Contains(resolved.Issues, i => i.Token == ThemeToken.WindowBackground);
    }

    [Fact]
    public void Identity_comes_from_the_theme_not_its_base()
    {
        var mine = new ThemeDefinition { Id = "mine", Name = "Mine", BaseThemeId = "light-plus", IsDark = false };

        var resolved = ThemeResolver.Resolve(mine, ThemeCatalog.Find);

        Assert.Equal("mine", resolved.Id);
        Assert.Equal("Mine", resolved.Name);
        Assert.False(resolved.IsDark);
        Assert.Equal(ThemeColor.Parse("#FFFFFF"), resolved[ThemeToken.WindowBackground]);
    }

    [Fact]
    public void No_lookup_at_all_still_resolves_against_the_default()
    {
        var mine = Theme("mine", "dark-plus", (ThemeToken.TextPrimary, "#010203"));

        var resolved = ThemeResolver.Resolve(mine);

        Assert.Equal(ThemeColor.Parse("#010203"), resolved[ThemeToken.TextPrimary]);
        Assert.Equal(ThemeColor.Parse("#1E1E1E"), resolved[ThemeToken.WindowBackground]);
    }

    [Fact]
    public void Pruning_drops_unknown_keys_bad_colours_and_no_op_overrides()
    {
        var dark = ThemeResolver.Resolve(ThemeCatalog.DarkPlus, ThemeCatalog.Find);
        var overrides = new Dictionary<string, string>
        {
            ["Theme.Not.AThing"] = "#FF0000",
            [ThemeToken.TextPrimary] = "not a colour",
            [ThemeToken.WindowBackground] = "#1e1e1e",   // same as Dark+, just cased differently
            [ThemeToken.AccentBackground] = "#C586C0",
        };

        var pruned = ThemeResolver.PruneOverrides(overrides, dark);

        Assert.Equal(new[] { ThemeToken.AccentBackground }, pruned.Keys);
        Assert.Equal("#C586C0", pruned[ThemeToken.AccentBackground]);
    }
}
