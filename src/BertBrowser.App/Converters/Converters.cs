using System.Globalization;
using System.Windows;
using System.Windows.Data;

namespace BertBrowser.App.Converters;

public sealed class BoolToVisibilityConverter : IValueConverter
{
    public bool Invert { get; set; }

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var b = value is true;
        if (Invert) b = !b;
        return b ? Visibility.Visible : Visibility.Collapsed;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

public sealed class InverseBoolConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is not true;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is not true;
}

public sealed class NullToCollapsedConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is null or "" ? Visibility.Collapsed : Visibility.Visible;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

/// <summary>
/// A <see cref="System.Windows.Media.Color"/> as a brush, for swatches in the theme editor whose
/// colour comes from a view model rather than from the palette.
/// </summary>
public sealed class ColorToBrushConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is System.Windows.Media.Color color
            ? new System.Windows.Media.SolidColorBrush(color)
            : System.Windows.Media.Brushes.Transparent;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

/// <summary>
/// A colour's alpha channel as an opacity. <see cref="System.Windows.Media.Effects.DropShadowEffect"/>
/// ignores the alpha of its <c>Color</c> and takes strength from <c>Opacity</c> instead, so the
/// shadow token carries its strength in the alpha and this hands it to the right property.
/// </summary>
public sealed class ColorAlphaToOpacityConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is System.Windows.Media.Color color ? color.A / 255.0 : 1.0;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

/// <summary>
/// Tree nesting depth to a left margin, so indentation lives inside the row and the
/// row's highlight/hit area can span the full width of the tree panel.
/// </summary>
public sealed class DepthToIndentConverter : IValueConverter
{
    public double IndentSize { get; set; } = 16;

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        new Thickness(value is int depth ? depth * IndentSize : 0, 0, 0, 0);

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
