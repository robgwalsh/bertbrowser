using System.Windows;
using System.Windows.Media;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// Points WPF's built-in <see cref="SystemColors"/> resource keys at the current theme.
/// </summary>
/// <remarks>
/// Anything we haven't explicitly retemplated falls through to WPF's default (Aero2) theme, which
/// resolves its colours through these keys with <c>DynamicResource</c> — so overriding them is a
/// cheap safety net that catches whatever a control template misses. It is what keeps the text
/// selection inside a <see cref="System.Windows.Controls.TextBox"/> from staying Windows blue on a
/// dark field, among other things. Unlike the token brushes these are replaced rather than mutated,
/// which is fine precisely because the consumers use <c>DynamicResource</c>.
/// </remarks>
internal static class ThemeSystemColors
{
    public static void Apply(ResourceDictionary target, ResolvedTheme theme)
    {
        SetBrush(target, SystemColors.WindowBrushKey, theme[ThemeToken.WindowBackground]);
        SetBrush(target, SystemColors.WindowTextBrushKey, theme[ThemeToken.TextPrimary]);
        SetBrush(target, SystemColors.ControlBrushKey, theme[ThemeToken.ButtonSecondaryBackground]);
        SetBrush(target, SystemColors.ControlTextBrushKey, theme[ThemeToken.TextPrimary]);
        SetBrush(target, SystemColors.ControlLightBrushKey, theme[ThemeToken.BorderSubtle]);
        SetBrush(target, SystemColors.ControlDarkBrushKey, theme[ThemeToken.BorderDefault]);
        SetBrush(target, SystemColors.GrayTextBrushKey, theme[ThemeToken.TextDisabled]);

        SetBrush(target, SystemColors.HighlightBrushKey, theme[ThemeToken.InputSelectionBackground]);
        SetBrush(target, SystemColors.HighlightTextBrushKey, theme[ThemeToken.InputSelectionForeground]);
        SetBrush(target, SystemColors.InactiveSelectionHighlightBrushKey, theme[ThemeToken.ListSelectedInactiveBackground]);
        SetBrush(target, SystemColors.InactiveSelectionHighlightTextBrushKey, theme[ThemeToken.ListSelectedInactiveForeground]);

        SetBrush(target, SystemColors.MenuBrushKey, theme[ThemeToken.MenuBackground]);
        SetBrush(target, SystemColors.MenuTextBrushKey, theme[ThemeToken.MenuForeground]);
        SetBrush(target, SystemColors.MenuHighlightBrushKey, theme[ThemeToken.MenuHoverBackground]);
        SetBrush(target, SystemColors.MenuBarBrushKey, theme[ThemeToken.MenuBackground]);

        SetBrush(target, SystemColors.InfoBrushKey, theme[ThemeToken.OverlayBackground]);
        SetBrush(target, SystemColors.InfoTextBrushKey, theme[ThemeToken.TextPrimary]);

        // A few default templates read the colour rather than the brush.
        target[SystemColors.HighlightColorKey] = ThemeTokenDictionary.ToMediaColor(theme[ThemeToken.InputSelectionBackground]);
        target[SystemColors.HighlightTextColorKey] = ThemeTokenDictionary.ToMediaColor(theme[ThemeToken.InputSelectionForeground]);
        target[SystemColors.WindowColorKey] = ThemeTokenDictionary.ToMediaColor(theme[ThemeToken.WindowBackground]);
        target[SystemColors.WindowTextColorKey] = ThemeTokenDictionary.ToMediaColor(theme[ThemeToken.TextPrimary]);
        target[SystemColors.GrayTextColorKey] = ThemeTokenDictionary.ToMediaColor(theme[ThemeToken.TextDisabled]);
    }

    private static void SetBrush(ResourceDictionary target, ResourceKey key, ThemeColor color) =>
        target[key] = new SolidColorBrush(ThemeTokenDictionary.ToMediaColor(color));
}
