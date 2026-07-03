using System.Globalization;
using System.Windows;
using System.Windows.Data;
using System.Windows.Media;

namespace BertStat.App.Converters;

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

/// <summary>null → true. Used to allow the indeterminate state only where it started indeterminate.</summary>
public sealed class IsNullConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is null;

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

public sealed class NullToCollapsedConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is null or "" ? Visibility.Collapsed : Visibility.Visible;

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

/// <summary>'#RRGGBB' string (or null) to a Brush for tag chips.</summary>
public sealed class TagColorConverter : IValueConverter
{
    private static readonly Brush DefaultBrush = new SolidColorBrush(Color.FromRgb(0x60, 0x7D, 0x8B));

    static TagColorConverter() => DefaultBrush.Freeze();

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is string s && s.Length > 0)
        {
            try
            {
                var brush = new SolidColorBrush((Color)ColorConverter.ConvertFromString(s));
                brush.Freeze();
                return brush;
            }
            catch (FormatException) { }
        }
        return DefaultBrush;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
