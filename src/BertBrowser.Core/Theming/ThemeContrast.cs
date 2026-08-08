namespace BertBrowser.Core.Theming;

/// <summary>
/// WCAG 2.1 contrast, used two ways: the test suite asserts every theme we ship is readable, and the
/// theme editor badges a token the user has just made illegible.
/// </summary>
public static class ThemeContrast
{
    /// <summary>WCAG AA for body text.</summary>
    public const double AaNormalText = 4.5;

    /// <summary>WCAG AA for large or incidental text.</summary>
    public const double AaLargeText = 3.0;

    /// <summary>
    /// Contrast ratio between 1 and 21. A translucent <paramref name="foreground"/> is composited
    /// over the background first — otherwise a token like the scrollbar thumb scores against an
    /// alpha it is never actually seen at.
    /// </summary>
    public static double Ratio(ThemeColor foreground, ThemeColor background)
    {
        var fg = foreground.CompositeOver(background).RelativeLuminance();
        var bg = background.RelativeLuminance();
        var (lighter, darker) = fg >= bg ? (fg, bg) : (bg, fg);
        return (lighter + 0.05) / (darker + 0.05);
    }

    public static double Ratio(ResolvedTheme theme, string foregroundToken, string backgroundToken) =>
        Ratio(theme[foregroundToken], theme[backgroundToken]);
}
