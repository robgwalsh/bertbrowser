using System.Windows;
using System.Windows.Data;
using System.Windows.Media;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// The app's palette: one <see cref="SolidColorBrush"/> per <see cref="ThemeToken"/>, plus the raw
/// <see cref="Color"/> beside it for the few places a brush won't do (effects, gradient stops).
/// </summary>
/// <remarks>
/// <para>
/// Declared by <c>Resources/Theme/Tokens.xaml</c>, which every dictionary that references a
/// <c>Theme.*</c> key merges. That is not redundancy: <c>StaticResource</c> searches the declaring
/// dictionary and its own merged children, but <b>not</b> its siblings, so a control dictionary that
/// merely sits next to the tokens in some parent's list resolves them to
/// <see cref="DependencyProperty.UnsetValue"/> and fails at load. Merging is therefore per-file, and
/// the instances share one set of brushes so it stays a single palette.
/// </para>
/// <para>
/// Construction must not touch disk or DI: <c>App.xaml</c>'s resources are parsed inside
/// <c>InitializeComponent()</c> in <c>Main</c>, before settings are loaded. The palette starts on
/// the default theme and <see cref="ThemeService"/> recolours it during startup, before any window
/// is shown.
/// </para>
/// <para>
/// Brushes are created once and recoloured in place, never replaced: every consumer holds these
/// exact instances, so a theme change repaints the whole UI with no restart and nothing to rebind.
/// See <see cref="ThemeBrush"/> for why each one is bound to a holder rather than assigned directly.
/// </para>
/// </remarks>
public sealed class ThemeTokenDictionary : ResourceDictionary
{
    private static readonly Dictionary<string, ThemeBrush> SharedBrushes =
        ThemeToken.All.ToDictionary(key => key, _ => new ThemeBrush(), StringComparer.Ordinal);

    private static readonly List<ThemeTokenDictionary> Instances = new();

    private static ResolvedTheme _current = ThemeResolver.Resolve(ThemeCatalog.Default, ThemeCatalog.Find);

    static ThemeTokenDictionary() => Recolour(_current);

    public ThemeTokenDictionary()
    {
        foreach (var key in ThemeToken.All) this[key] = SharedBrushes[key].Brush;
        Instances.Add(this);
        Populate(this, _current);
    }

    /// <summary>
    /// Resource key for a token's raw <see cref="Color"/>. Colours are value types and cannot be
    /// mutated in place, so the handful of consumers that need one (a <c>DropShadowEffect</c>, say)
    /// must bind it with <c>DynamicResource</c> rather than <c>StaticResource</c>.
    /// </summary>
    public static string ColorKey(string token) => token + ".Value";

    /// <summary>Recolours the shared palette. Every brush handed out so far follows.</summary>
    public static void ApplyTheme(ResolvedTheme theme)
    {
        _current = theme;
        Recolour(theme);
        foreach (var instance in Instances) Populate(instance, theme);
    }

    public static Color ToMediaColor(ThemeColor color) =>
        Color.FromArgb(color.A, color.R, color.G, color.B);

    public static ThemeColor ToThemeColor(Color color) =>
        new(color.A, color.R, color.G, color.B);

    private static void Recolour(ResolvedTheme theme)
    {
        foreach (var key in ThemeToken.All) SharedBrushes[key].Color = ToMediaColor(theme[key]);
    }

    private static void Populate(ResourceDictionary target, ResolvedTheme theme)
    {
        foreach (var key in ThemeToken.All) target[ColorKey(key)] = ToMediaColor(theme[key]);
        ThemeSystemColors.Apply(target, theme);
    }

    /// <summary>
    /// A recolourable brush.
    /// </summary>
    /// <remarks>
    /// The obvious implementation — hold a <see cref="SolidColorBrush"/> and assign its
    /// <see cref="SolidColorBrush.Color"/> — does not survive contact with WPF: a
    /// <see cref="ResourceDictionary"/> seals its <see cref="Freezable"/> values when the
    /// <see cref="Application"/> takes ownership of it, so the brush is frozen before the first
    /// theme is ever applied and every later assignment throws.
    /// <para>
    /// Binding the colour instead sidesteps that entirely: a freezable carrying an expression
    /// reports <c>CanFreeze == false</c>, so sealing skips it and the brush stays live for the
    /// process lifetime. Recolouring then means setting <see cref="Color"/> on this holder, which
    /// the binding pushes into the brush every consumer already points at.
    /// </para>
    /// </remarks>
    private sealed class ThemeBrush : DependencyObject
    {
        public static readonly DependencyProperty ColorProperty = DependencyProperty.Register(
            nameof(Color), typeof(Color), typeof(ThemeBrush), new PropertyMetadata(Colors.Transparent));

        public ThemeBrush()
        {
            Brush = new SolidColorBrush();
            BindingOperations.SetBinding(Brush, SolidColorBrush.ColorProperty,
                new Binding(nameof(Color)) { Source = this, Mode = BindingMode.OneWay });
        }

        public SolidColorBrush Brush { get; }

        public Color Color
        {
            get => (Color)GetValue(ColorProperty);
            set => SetValue(ColorProperty, value);
        }
    }
}
