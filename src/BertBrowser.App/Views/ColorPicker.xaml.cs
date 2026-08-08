using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using BertBrowser.App.Theming;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// A saturation/value square with hue and alpha strips. WPF ships no colour picker, and the theme
/// editor needs one that understands alpha — several tokens (scrollbar thumb, rubber band, shadow)
/// are translucent by design.
/// </summary>
public partial class ColorPicker : UserControl
{
    public static readonly DependencyProperty SelectedColorProperty = DependencyProperty.Register(
        nameof(SelectedColor), typeof(Color), typeof(ColorPicker),
        new FrameworkPropertyMetadata(Colors.Black,
            FrameworkPropertyMetadataOptions.BindsTwoWayByDefault, OnSelectedColorChanged));

    private double _hue;
    private double _saturation;
    private double _value;
    private byte _alpha = 0xFF;

    private Border? _dragging;
    private bool _updating;

    public ColorPicker()
    {
        InitializeComponent();

        var checkerboard = BuildCheckerboard();
        Checkerboard.Fill = checkerboard;
        PreviewCheckerboard.Fill = checkerboard;

        Loaded += (_, _) => Redraw();
    }

    public Color SelectedColor
    {
        get => (Color)GetValue(SelectedColorProperty);
        set => SetValue(SelectedColorProperty, value);
    }

    private static void OnSelectedColorChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        if (d is ColorPicker picker && !picker._updating) picker.AdoptExternalColor((Color)e.NewValue);
    }

    /// <summary>
    /// Takes hue/saturation/value from a colour set from outside. Grey and black have no meaningful
    /// hue, so the existing one is kept — otherwise dragging the value slider down to black and back
    /// up would snap the picker to red.
    /// </summary>
    private void AdoptExternalColor(Color color)
    {
        var (h, s, v) = ThemeTokenDictionary.ToThemeColor(color).ToHsv();
        if (s > 0) _hue = h;
        _saturation = s;
        _value = v;
        _alpha = color.A;
        Redraw();
    }

    private void Area_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is not Border area) return;
        _dragging = area;
        area.CaptureMouse();
        UpdateFromPoint(area, e.GetPosition(area));
    }

    private void Area_MouseMove(object sender, MouseEventArgs e)
    {
        if (_dragging is null || e.LeftButton != MouseButtonState.Pressed) return;
        UpdateFromPoint(_dragging, e.GetPosition(_dragging));
    }

    private void Area_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if (sender is Border area) area.ReleaseMouseCapture();
        _dragging = null;
    }

    private void UpdateFromPoint(Border area, Point point)
    {
        var width = Math.Max(1, area.ActualWidth);
        var height = Math.Max(1, area.ActualHeight);
        var x = Math.Clamp(point.X / width, 0, 1);
        var y = Math.Clamp(point.Y / height, 0, 1);

        if (ReferenceEquals(area, SaturationValueArea))
        {
            _saturation = x;
            _value = 1 - y;
        }
        else if (ReferenceEquals(area, HueArea))
        {
            _hue = x * 360;
        }
        else if (ReferenceEquals(area, AlphaArea))
        {
            _alpha = (byte)Math.Round(x * 255);
        }

        Commit();
    }

    private void HexBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        if (_updating) return;
        if (!ThemeColor.TryParse(HexBox.Text, out var parsed)) return;

        AdoptExternalColor(ThemeTokenDictionary.ToMediaColor(parsed));
        Commit(skipHexBox: true);
    }

    private void Commit(bool skipHexBox = false)
    {
        var color = ThemeTokenDictionary.ToMediaColor(ThemeColor.FromHsv(_hue, _saturation, _value, _alpha));

        _updating = true;
        SelectedColor = color;
        _updating = false;

        Redraw(skipHexBox);
    }

    private void Redraw(bool skipHexBox = false)
    {
        var opaque = ThemeColor.FromHsv(_hue, _saturation, _value);
        var full = new ThemeColor(_alpha, opaque.R, opaque.G, opaque.B);

        HueFill.Fill = new SolidColorBrush(ThemeTokenDictionary.ToMediaColor(ThemeColor.FromHsv(_hue, 1, 1)));
        PreviewFill.Fill = new SolidColorBrush(ThemeTokenDictionary.ToMediaColor(full));
        AlphaFill.Fill = new LinearGradientBrush(
            ThemeTokenDictionary.ToMediaColor(new ThemeColor(0, opaque.R, opaque.G, opaque.B)),
            ThemeTokenDictionary.ToMediaColor(opaque),
            new Point(0, 0),
            new Point(1, 0));

        PositionThumbs();

        if (!skipHexBox)
        {
            _updating = true;
            HexBox.Text = full.ToHex();
            _updating = false;
        }
    }

    private void PositionThumbs()
    {
        if (SaturationValueArea.ActualWidth > 0)
        {
            Canvas.SetLeft(SaturationValueThumb, _saturation * SaturationValueArea.ActualWidth - 5.5);
            Canvas.SetTop(SaturationValueThumb, (1 - _value) * SaturationValueArea.ActualHeight - 5.5);
        }
        if (HueArea.ActualWidth > 0)
            Canvas.SetLeft(HueThumb, _hue / 360 * HueArea.ActualWidth - 1.5);
        if (AlphaArea.ActualWidth > 0)
            Canvas.SetLeft(AlphaThumb, _alpha / 255.0 * AlphaArea.ActualWidth - 1.5);
    }

    private static Brush BuildCheckerboard()
    {
        var cell = new DrawingGroup();
        cell.Children.Add(new GeometryDrawing(
            new SolidColorBrush(Color.FromRgb(0x9E, 0x9E, 0x9E)), null,
            new RectangleGeometry(new Rect(0, 0, 8, 8))));
        cell.Children.Add(new GeometryDrawing(
            new SolidColorBrush(Color.FromRgb(0xD0, 0xD0, 0xD0)), null,
            new RectangleGeometry(new Rect(0, 0, 4, 4))));
        cell.Children.Add(new GeometryDrawing(
            new SolidColorBrush(Color.FromRgb(0xD0, 0xD0, 0xD0)), null,
            new RectangleGeometry(new Rect(4, 4, 4, 4))));

        var brush = new DrawingBrush(cell)
        {
            TileMode = TileMode.Tile,
            Viewport = new Rect(0, 0, 8, 8),
            ViewportUnits = BrushMappingMode.Absolute,
            Stretch = Stretch.None,
        };
        brush.Freeze();
        return brush;
    }
}
