namespace BertBrowser.Core.Theming;

/// <summary>
/// What Windows is currently set to, as far as theming cares.
/// </summary>
/// <remarks>
/// Two independent switches rather than three states, and deliberately so: high contrast decides
/// which theme is used, but the light/dark bit is still read and kept underneath it. That is what
/// lets turning high contrast <em>off</em> land back on the right slot with nothing to re-read and
/// no restart.
/// </remarks>
public readonly record struct SystemAppearance(bool IsDark, bool IsHighContrast)
{
    public static SystemAppearance Light { get; } = new(false, false);

    public static SystemAppearance Dark { get; } = new(true, false);

    /// <summary>Windows ships a light and a dark high-contrast scheme and we answer both with the
    /// one accessibility theme, so this carries a dark bit only for what happens when it goes off.
    /// </summary>
    public static SystemAppearance HighContrast { get; } = new(true, true);
}
