using System.Globalization;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// The treemap: one folder's children as rectangles whose areas are their sizes.
/// </summary>
/// <remarks>
/// <para>
/// A single element that draws every tile in <see cref="OnRender"/>, rather than a Canvas holding a
/// Rectangle and a TextBlock per item. Two elements per tile means full layout, hit-testing and
/// bookkeeping for each, which is visibly slow past a few hundred; drawing straight into a
/// <see cref="DrawingContext"/> has no per-tile visual at all, so there is no ceiling to design
/// around and nothing to virtualize.
/// </para>
/// <para>
/// Hit-testing is a point-in-rect walk over the layout the Core algorithm already returned, which
/// is the other half of that trade: without child elements, nothing else would know what was
/// clicked.
/// </para>
/// </remarks>
public sealed class TreemapCanvas : FrameworkElement
{
    /// <summary>Tiles below this get no label — text that has to be clipped tells nobody
    /// anything, and measuring it for every tile is most of the drawing cost.</summary>
    private const double MinLabelWidth = 44;
    private const double MinLabelHeight = 16;

    /// <summary>
    /// Past this many, the rest collapse into one "N smaller items" tile.
    /// </summary>
    /// <remarks>
    /// Not only a performance guard: four thousand one-pixel slivers convey nothing anyone can
    /// read, click or compare, so folding them is better information design as well as cheaper.
    /// </remarks>
    private const int MaxTiles = 300;

    private IReadOnlyList<DiskUsageTileViewModel> _items = [];
    private IReadOnlyList<TreemapRect> _rects = [];
    private IReadOnlyList<Brush> _fills = [];
    private Brush _labelBrush = Brushes.White;
    private Pen _separator = new(Brushes.Black, 1);
    private int _foldedCount;
    private long _foldedBytes;
    private int _hovered = -1;

    /// <summary>Raised when a tile is clicked, with the item it stands for — null for the folded
    /// "smaller items" tile, which is not one thing.</summary>
    public event Action<DiskUsageTileViewModel?>? TileActivated;

    public TreemapCanvas()
    {
        ClipToBounds = true;
        Focusable = false;
    }

    /// <summary>Points the map at a new set of children and repaints.</summary>
    public void SetItems(IReadOnlyList<DiskUsageTileViewModel> items)
    {
        // Only measured children get area. An unknown size has no share to draw, which is the same
        // rule the list beside it follows by leaving the bar empty.
        var measured = items.Where(i => !i.IsUnknown && i.Node.SizeBytes > 0).ToList();

        _foldedCount = Math.Max(0, measured.Count - MaxTiles);
        if (_foldedCount > 0)
        {
            _foldedBytes = measured.Skip(MaxTiles).Sum(i => i.Node.SizeBytes ?? 0);
            measured = measured.Take(MaxTiles).ToList();
        }
        else
        {
            _foldedBytes = 0;
        }

        _items = measured;
        _hovered = -1;
        Layout();
        InvalidateVisual();
    }

    /// <summary>
    /// Rebuilds the brushes for the current theme.
    /// </summary>
    /// <remarks>
    /// These are ordinary brushes replaced wholesale, <em>not</em> the token brushes from
    /// <see cref="ThemeTokenDictionary"/> — which are recoloured in place and must never be frozen.
    /// Freezing is right here, and cheap: a new set is built on every theme change anyway.
    /// </remarks>
    public void ApplyTheme(ResolvedTheme theme)
    {
        ArgumentNullException.ThrowIfNull(theme);

        var ramp = TreemapPalette.Ramp(theme, 12);
        var fills = new List<Brush>(ramp.Count);
        foreach (var colour in ramp)
        {
            var brush = new SolidColorBrush(ThemeTokenDictionary.ToMediaColor(colour));
            brush.Freeze();
            fills.Add(brush);
        }
        _fills = fills;

        _labelBrush = Frozen(theme[ThemeToken.TextOnAccent]);
        _separator = new Pen(Frozen(theme[ThemeToken.BorderSubtle]), 1);
        _separator.Freeze();

        InvalidateVisual();
    }

    private static SolidColorBrush Frozen(ThemeColor colour)
    {
        var brush = new SolidColorBrush(ThemeTokenDictionary.ToMediaColor(colour));
        brush.Freeze();
        return brush;
    }

    protected override void OnRenderSizeChanged(SizeChangedInfo info)
    {
        base.OnRenderSizeChanged(info);
        Layout();
    }

    private void Layout()
    {
        var weights = _items.Select(i => (double)(i.Node.SizeBytes ?? 0)).ToList();
        if (_foldedCount > 0) weights.Add(_foldedBytes);

        _rects = TreemapLayout.Arrange(weights, ActualWidth, ActualHeight);
    }

    protected override void OnRender(DrawingContext dc)
    {
        base.OnRender(dc);
        if (_rects.Count == 0 || _fills.Count == 0) return;

        var typeface = new Typeface(
            new FontFamily("Segoe UI"), FontStyles.Normal, FontWeights.Normal, FontStretches.Normal);
        var pixelsPerDip = VisualTreeHelper.GetDpi(this).PixelsPerDip;

        foreach (var rect in _rects)
        {
            var bounds = new Rect(rect.X, rect.Y, rect.Width, rect.Height);
            var folded = rect.Index >= _items.Count;

            var fill = _fills[rect.Index % _fills.Count];
            dc.DrawRectangle(fill, _separator, bounds);

            // The hovered tile gets an outline rather than a different fill: changing the colour
            // would misreport its size band.
            if (rect.Index == _hovered)
                dc.DrawRectangle(null, new Pen(_labelBrush, 2), Rect.Inflate(bounds, -1, -1));

            if (bounds.Width < MinLabelWidth || bounds.Height < MinLabelHeight) continue;

            var caption = folded
                ? $"{_foldedCount} smaller items"
                : _items[rect.Index].Name;

            var text = new FormattedText(
                caption, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight,
                typeface, 12, _labelBrush, pixelsPerDip)
            {
                MaxTextWidth = Math.Max(1, bounds.Width - 8),
                MaxLineCount = 1,
                Trimming = TextTrimming.CharacterEllipsis,
            };

            dc.DrawText(text, new Point(bounds.X + 4, bounds.Y + 3));
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);

        var index = HitTest(e.GetPosition(this));
        if (index == _hovered) return;

        _hovered = index;
        ToolTip = index >= 0 && index < _items.Count
            ? $"{_items[index].Name} — {_items[index].SizeText}"
            : null;
        InvalidateVisual();
    }

    protected override void OnMouseLeave(MouseEventArgs e)
    {
        base.OnMouseLeave(e);
        if (_hovered < 0) return;

        _hovered = -1;
        ToolTip = null;
        InvalidateVisual();
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonUp(e);

        var index = HitTest(e.GetPosition(this));
        if (index < 0) return;

        TileActivated?.Invoke(index < _items.Count ? _items[index] : null);
    }

    /// <summary>Which tile a point is in, or -1. Linear over the layout list, which is bounded by
    /// <see cref="MaxTiles"/> and only ever walked on a mouse event.</summary>
    private int HitTest(Point point)
    {
        foreach (var rect in _rects)
        {
            if (point.X >= rect.X && point.X < rect.Right &&
                point.Y >= rect.Y && point.Y < rect.Bottom)
                return rect.Index;
        }
        return -1;
    }

    /// <summary>Takes whatever it is given: the map fills its cell, and an empty one draws
    /// nothing.</summary>
    protected override Size MeasureOverride(Size availableSize) => new(
        double.IsInfinity(availableSize.Width) ? 0 : availableSize.Width,
        double.IsInfinity(availableSize.Height) ? 0 : availableSize.Height);
}
