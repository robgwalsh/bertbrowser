using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

/// <summary>
/// The treemap's fills are derived from each theme's accent rather than authored, so the contrast
/// guarantee the shipped tokens get from <see cref="ThemeCatalogTests"/> has to be earned here by
/// rule instead. Every built-in is run through it, including the light-accent ones.
/// </summary>
public class TreemapPaletteTests
{
    public static TheoryData<string> BuiltInIds()
    {
        var data = new TheoryData<string>();
        foreach (var theme in ThemeCatalog.BuiltIns) data.Add(theme.Id);
        return data;
    }

    private static ResolvedTheme Resolve(string id) =>
        ThemeResolver.Resolve(ThemeCatalog.Find(id)!, ThemeCatalog.Find);

    /// <summary>
    /// The one that matters, and the reason the fills are adjusted rather than just rotated: at a
    /// fixed saturation and value, yellows and greens come out far lighter than blues, so a plain
    /// hue rotation produces tiles the label disappears on. Delete the contrast pull in
    /// <c>TreemapPalette.Legible</c> and this goes red.
    /// </summary>
    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Every_fill_is_legible_under_the_label_colour(string id)
    {
        var theme = Resolve(id);
        var label = theme[ThemeToken.TextOnAccent];

        foreach (var fill in TreemapPalette.Ramp(theme, 12))
        {
            var ratio = ThemeContrast.Ratio(label, fill);
            Assert.True(
                ratio >= ThemeContrast.AaNormalText,
                $"{id}: label on {fill.ToHex()} is only {ratio:F2}:1");
        }
    }

    /// <summary>Neighbouring tiles have to be told apart, which is the entire job of the hue
    /// rotation.</summary>
    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void Adjacent_fills_are_visibly_different(string id)
    {
        var ramp = TreemapPalette.Ramp(Resolve(id), 8);

        for (var i = 1; i < ramp.Count; i++)
        {
            Assert.True(
                Distance(ramp[i - 1], ramp[i]) > 20,
                $"{id}: {ramp[i - 1].ToHex()} and {ramp[i].ToHex()} are nearly the same colour");
        }
    }

    [Theory]
    [MemberData(nameof(BuiltInIds))]
    public void The_ramp_is_deterministic(string id)
    {
        var theme = Resolve(id);

        Assert.Equal(TreemapPalette.Ramp(theme, 6), TreemapPalette.Ramp(theme, 6));
    }

    [Fact]
    public void The_requested_count_is_what_comes_back()
    {
        var theme = Resolve(ThemeCatalog.BuiltIns[0].Id);

        Assert.Equal(5, TreemapPalette.Ramp(theme, 5).Count);
        Assert.Empty(TreemapPalette.Ramp(theme, 0));
        Assert.Empty(TreemapPalette.Ramp(theme, -1));
    }

    /// <summary>
    /// A theme whose accent is nearly grey has no hue to rotate, so without a floor on saturation
    /// the whole ramp would come back as one colour repeated.
    /// </summary>
    [Fact]
    public void A_greyscale_accent_still_produces_distinguishable_fills()
    {
        var grey = ThemeResolver.Resolve(
            new ThemeDefinition
            {
                Id = "grey-test",
                Name = "Grey",
                BaseThemeId = ThemeCatalog.DarkPlus.Id,
                Colors = new Dictionary<string, string>
                {
                    [ThemeToken.AccentBackground] = "#FF808080",
                },
            },
            ThemeCatalog.Find);

        var ramp = TreemapPalette.Ramp(grey, 6);

        for (var i = 1; i < ramp.Count; i++)
            Assert.True(Distance(ramp[i - 1], ramp[i]) > 20, "the ramp collapsed to one colour");
    }

    private static double Distance(ThemeColor a, ThemeColor b)
    {
        double dr = a.R - b.R, dg = a.G - b.G, db = a.B - b.B;
        return Math.Sqrt(dr * dr + dg * dg + db * db);
    }
}
