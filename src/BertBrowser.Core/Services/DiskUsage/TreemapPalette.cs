using BertBrowser.Core.Theming;

namespace BertBrowser.Core.Services.DiskUsage;

/// <summary>
/// The treemap's fill colours, derived from whatever theme is loaded rather than authored.
/// </summary>
/// <remarks>
/// <para>
/// Deriving rather than adding tokens is a deliberate trade. A ramp of eight fills would be eight
/// new <see cref="ThemeToken"/>s that every root theme must define and every built-in must pass the
/// contrast suite with — a great deal of hand-picked colour for something a rule can produce, and a
/// tax on every future theme. Rotating hue from the theme's own accent gives a palette that follows
/// all twenty built-ins <em>and</em> any theme a user writes, for free.
/// </para>
/// <para>
/// The rotation is the golden angle, which is what keeps successive hues far apart no matter how
/// many are asked for — stepping by a fixed fraction of the circle instead makes the first and last
/// entries collide as soon as the count divides evenly into 360.
/// </para>
/// </remarks>
public static class TreemapPalette
{
    /// <summary>≈137.507°, the angle that spreads any number of samples most evenly.</summary>
    private const double GoldenAngle = 137.50776405003785;

    /// <summary>
    /// <paramref name="count"/> fills for <paramref name="theme"/>, each legible under the same
    /// text colour an accent-coloured surface uses.
    /// </summary>
    public static IReadOnlyList<ThemeColor> Ramp(ResolvedTheme theme, int count)
    {
        ArgumentNullException.ThrowIfNull(theme);
        if (count <= 0) return [];

        var accent = theme[ThemeToken.AccentBackground];
        var label = theme[ThemeToken.TextOnAccent];
        var (hue, saturation, value) = accent.ToHsv();

        // A near-grey accent has no meaningful hue to rotate, so the ramp would come out as one
        // colour repeated. Give it enough saturation to be distinguishable.
        if (saturation < 0.15) saturation = 0.45;

        var ramp = new List<ThemeColor>(count);
        for (var i = 0; i < count; i++)
        {
            var h = (hue + GoldenAngle * i) % 360;
            ramp.Add(Legible(ThemeColor.FromHsv(h, saturation, value), label, value));
        }
        return ramp;
    }

    /// <summary>
    /// Darkens or lightens a fill until the label on it clears AA.
    /// </summary>
    /// <remarks>
    /// Hue rotation alone is not enough: at a fixed saturation and value, yellows and greens are far
    /// lighter than blues and purples, so a ramp built from a light accent produces entries that
    /// white text vanishes on. Which direction to walk is decided by the label rather than assumed —
    /// the light-accent themes (Ayu Mirage, Cobalt2) deliberately use a <em>dark</em>
    /// <c>Text.OnAccent</c>, and lightening is what helps there.
    /// </remarks>
    private static ThemeColor Legible(ThemeColor fill, ThemeColor label, double startValue)
    {
        if (ThemeContrast.Ratio(label, fill) >= ThemeContrast.AaNormalText) return fill;

        // Move away from the label: a light label wants a darker fill, and the reverse.
        var towardsDark = label.RelativeLuminance() > 0.5;
        var (h, startSaturation, _) = fill.ToHsv();

        const int Steps = 24;
        var last = fill;

        for (var step = 1; step <= Steps; step++)
        {
            var t = (double)step / Steps;

            // Going lighter, saturation comes down with value. Raising value alone tops out at a
            // vivid colour that is still too dark for a dark label on several hues, and pushing
            // further just saturates to white — which is what made every light-accent theme's ramp
            // collapse to a row of identical white tiles. A pastel gets light while keeping the hue
            // that tells one tile from its neighbour.
            var v = towardsDark
                ? startValue * (1 - t * 0.9)
                : startValue + (1 - startValue) * t;
            var s = towardsDark
                ? startSaturation
                : startSaturation * (1 - t * 0.7);

            last = ThemeColor.FromHsv(h, Math.Clamp(s, 0, 1), Math.Clamp(v, 0, 1));
            if (ThemeContrast.Ratio(label, last) >= ThemeContrast.AaNormalText)
                return last;
        }

        // Nothing on this hue cleared it. Return the far end of the walk rather than plain black or
        // white: it is the most legible this hue gets, and it still differs from its neighbours,
        // which pure white would not.
        return last;
    }
}
