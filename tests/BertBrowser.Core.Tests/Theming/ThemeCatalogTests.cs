using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

/// <summary>
/// Guards the themes we ship. These are the invariants to mutate when changing this code: delete a
/// token from Dark+ and <see cref="Root_themes_define_every_token"/> goes red; set
/// <c>Theme.Text.Secondary</c> to something near the background and the contrast suite goes red.
/// </summary>
public class ThemeCatalogTests
{
    public static TheoryData<string> BuiltInIds()
    {
        var data = new TheoryData<string>();
        foreach (var theme in ThemeCatalog.BuiltIns) data.Add(theme.Id);
        return data;
    }

    public static TheoryData<string> RootIds()
    {
        var data = new TheoryData<string>();
        foreach (var theme in ThemeCatalog.Roots) data.Add(theme.Id);
        return data;
    }

    private static ResolvedTheme Resolve(string id) =>
        ThemeResolver.Resolve(ThemeCatalog.Find(id)!, ThemeCatalog.Find);

    [Theory]
    [MemberData(nameof(RootIds))]
    public void Root_themes_define_every_token(string id)
    {
        var theme = ThemeCatalog.Find(id)!;
        var missing = ThemeToken.All.Where(key => !theme.Colors.ContainsKey(key)).ToList();
        Assert.Empty(missing);
    }

    [Theory]
    [MemberData(nameof(RootIds))]
    public void Root_themes_have_no_base(string id)
    {
        Assert.Null(ThemeCatalog.Find(id)!.BaseThemeId);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Built_in_colours_all_parse(string id)
    {
        var unparseable = ThemeCatalog.Find(id)!.Colors
            .Where(pair => !ThemeColor.TryParse(pair.Value, out _))
            .Select(pair => $"{pair.Key} = {pair.Value}")
            .ToList();
        Assert.Empty(unparseable);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Built_in_themes_only_use_known_tokens(string id)
    {
        var unknown = ThemeCatalog.Find(id)!.Colors.Keys.Where(key => !ThemeToken.IsKnown(key)).ToList();
        Assert.Empty(unknown);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Built_in_themes_resolve_cleanly_and_completely(string id)
    {
        var resolved = Resolve(id);

        Assert.Empty(resolved.Issues);
        foreach (var key in ThemeToken.All)
        {
            Assert.True(resolved.Colors.ContainsKey(key), $"{id} is missing {key}");
        }
    }

    [Fact]
    public void Built_in_ids_and_names_are_unique()
    {
        Assert.Equal(ThemeCatalog.BuiltIns.Count,
            ThemeCatalog.BuiltIns.Select(t => t.Id).Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.Equal(ThemeCatalog.BuiltIns.Count,
            ThemeCatalog.BuiltIns.Select(t => t.Name).Distinct(StringComparer.OrdinalIgnoreCase).Count());
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void IsDark_matches_the_window_background(string id)
    {
        var resolved = Resolve(id);
        var luminance = resolved[ThemeToken.WindowBackground].RelativeLuminance();

        Assert.Equal(resolved.IsDark, luminance < 0.5);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Body_text_is_readable_on_every_surface(string id)
    {
        var theme = Resolve(id);

        AssertContrast(theme, ThemeToken.TextPrimary, ThemeToken.WindowBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.TextPrimary, ThemeToken.SurfaceBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.MenuForeground, ThemeToken.MenuBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.InputForeground, ThemeToken.InputBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.TitleBarForeground, ThemeToken.TitleBarBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.ToolbarForeground, ThemeToken.ToolbarBackground, ThemeContrast.AaNormalText);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Text_on_a_coloured_background_is_readable(string id)
    {
        var theme = Resolve(id);

        AssertContrast(theme, ThemeToken.ListSelectedForeground, ThemeToken.ListSelectedBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.ListSelectedInactiveForeground, ThemeToken.ListSelectedInactiveBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.StatusBarForeground, ThemeToken.StatusBarBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.MenuHoverForeground, ThemeToken.MenuHoverBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.AccentForeground, ThemeToken.AccentBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.TabActiveForeground, ThemeToken.TabActiveBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.InputSelectionForeground, ThemeToken.InputSelectionBackground, ThemeContrast.AaNormalText);
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Incidental_text_clears_the_large_text_bar(string id)
    {
        var theme = Resolve(id);

        AssertContrast(theme, ThemeToken.TextSecondary, ThemeToken.WindowBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.TextMuted, ThemeToken.SurfaceBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.TextPlaceholder, ThemeToken.InputBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.TabInactiveForeground, ThemeToken.TabInactiveBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.ListHeaderForeground, ThemeToken.ListHeaderBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.MenuGestureForeground, ThemeToken.MenuBackground, ThemeContrast.AaLargeText);
        AssertContrast(theme, ThemeToken.TreeChevronForeground, ThemeToken.SurfaceBackground, ThemeContrast.AaLargeText);
    }

    /// <summary>A row must not lose its text the moment it is hovered or selected.</summary>
    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Row_text_survives_hover(string id)
    {
        var theme = Resolve(id);
        AssertContrast(theme, ThemeToken.TextPrimary, ThemeToken.ListHoverBackground, ThemeContrast.AaNormalText);
        AssertContrast(theme, ThemeToken.ListSelectedForeground, ThemeToken.ListSelectedHoverBackground, ThemeContrast.AaNormalText);
    }

    /// <summary>
    /// The scrollbar thumb is translucent by design, so what matters is how it reads once composited
    /// over the surface behind it.
    /// </summary>
    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void The_scrollbar_thumb_is_visible_against_the_list(string id)
    {
        var theme = Resolve(id);
        var ratio = ThemeContrast.Ratio(theme[ThemeToken.ScrollBarThumb], theme[ThemeToken.ListBackground]);
        Assert.True(ratio >= 1.4, $"{id}: scrollbar thumb is only {ratio:0.00}:1 against the list");
    }

    private static void AssertContrast(ResolvedTheme theme, string foreground, string background, double minimum)
    {
        var ratio = ThemeContrast.Ratio(theme, foreground, background);
        Assert.True(ratio >= minimum,
            $"{theme.Id}: {foreground} on {background} is {ratio:0.00}:1, below the {minimum:0.0}:1 minimum");
    }
}
