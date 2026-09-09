using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Threading;

namespace BertBrowser.App.Views;

/// <summary>
/// The thumbnail view's panel: tiles that wrap, and only the ones on screen are built.
/// </summary>
/// <remarks>
/// <para>
/// <b>Why this exists rather than <c>WrapPanel</c>, which is what it replaced.</b> A
/// <c>WrapPanel</c> is not a <see cref="VirtualizingPanel"/>, so the list built a container for
/// every row — and every container asks for a thumbnail the moment it is realized. Measured over a
/// flat branch view of a thousand video files: the details list took 3.9 s, the same listing as
/// tiles never finished inside a four-minute watchdog. The panel alone, with the thumbnail requests
/// taken out of it, still cost six seconds a thousand rows and grew worse than linearly, because
/// every arriving thumbnail re-measured all of them.
/// </para>
/// <para>
/// <b>The layout is arithmetic, not measurement.</b> Nothing has to be realized to know where it
/// goes: a media item is a tile of a known size and anything else is a full-width row, so the lines
/// can be laid out for a hundred thousand items in one pass over an array and only the handful
/// inside the viewport ever become elements. That is what makes this cheap enough to redo whenever
/// the panel is resized.
/// </para>
/// <para>
/// <b>Tiles must therefore be uniform</b>, which is why the tile template's caption has a fixed
/// height rather than a maximum one. The assumed size is checked against the first tile actually
/// measured and corrected once if the template drifts from it, so a change to the template costs a
/// second layout pass rather than a silently misaligned grid.
/// </para>
/// </remarks>
public sealed class VirtualizingWrapPanel : VirtualizingPanel, IScrollInfo
{
    /// <summary>
    /// The floor under one "line" of scrolling — what a pixel-scrolling
    /// <c>VirtualizingStackPanel</c> uses.
    /// </summary>
    /// <remarks>
    /// Deliberately not a whole line of tiles: <c>ScrollSpeed</c> turns one wheel notch into
    /// several <see cref="LineDown"/> calls and then multiplies that by the user's setting, so a
    /// tile-sized step would fly a dozen rows per notch.
    /// </remarks>
    private const double ScrollLineDelta = 16;

    /// <summary>What a tile costs beyond its picture: the template's margins, and the caption under
    /// it. Only ever an opening guess — <see cref="_measuredCell"/> replaces it with the real thing
    /// as soon as one tile has been measured.</summary>
    private const double TileChromeX = 12;
    private const double TileChromeY = 44;

    /// <summary>The full-width row a folder or a non-media file gets, until one has been measured.</summary>
    private const double DefaultRowHeight = 24;

    public static readonly DependencyProperty TileWidthProperty = DependencyProperty.Register(
        nameof(TileWidth), typeof(double), typeof(VirtualizingWrapPanel),
        new FrameworkPropertyMetadata(96d, FrameworkPropertyMetadataOptions.AffectsMeasure, OnTileSizeChanged));

    public static readonly DependencyProperty TileHeightProperty = DependencyProperty.Register(
        nameof(TileHeight), typeof(double), typeof(VirtualizingWrapPanel),
        new FrameworkPropertyMetadata(96d, FrameworkPropertyMetadataOptions.AffectsMeasure, OnTileSizeChanged));

    /// <summary>The picture's width — the same value the tile template binds to.</summary>
    public double TileWidth
    {
        get => (double)GetValue(TileWidthProperty);
        set => SetValue(TileWidthProperty, value);
    }

    /// <summary>The picture's height — likewise.</summary>
    public double TileHeight
    {
        get => (double)GetValue(TileHeightProperty);
        set => SetValue(TileHeightProperty, value);
    }

    /// <summary>The slider moved, so whatever was measured is about to be the wrong size.</summary>
    private static void OnTileSizeChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var panel = (VirtualizingWrapPanel)d;
        panel._measuredCell = null;
        panel._corrected = false;
    }

    /// <summary>One run of items on one line. A tile line holds <see cref="Count"/> of them; a row
    /// line is one full-width item.</summary>
    private readonly record struct Line(int First, int Count, double Top, double Height, bool IsRow);

    private readonly List<Line> _lines = [];

    /// <summary>Which line each item landed on, so arranging one is a lookup rather than a search.</summary>
    private int[] _lineOfItem = [];

    private Size _cell;                 // the size one tile occupies, chrome included
    private Size? _measuredCell;        // what a real tile turned out to be
    private double _rowHeight = DefaultRowHeight;
    private double? _measuredRowHeight;
    private bool _corrected;            // one re-layout per size change, never a loop

    private int _columns = 1;
    private int _firstVisible;
    private int _lastVisible = -1;

    private Size _extent;
    private Size _viewport;
    private Vector _offset;

    // --- Layout ---

    protected override Size MeasureOverride(Size availableSize)
    {
        // Touching this is what initialises the item container generator; the generator is null
        // until something has asked the panel for its children.
        var children = InternalChildren;
        var owner = ItemsControl.GetItemsOwner(this);
        if (owner is null) return default;

        var items = owner.Items;
        var width = double.IsInfinity(availableSize.Width) ? _viewport.Width : availableSize.Width;
        var height = double.IsInfinity(availableSize.Height) ? _viewport.Height : availableSize.Height;

        BuildLines(items, width);

        var viewport = new Size(width, height);
        var extent = new Size(width, Math.Max(_extent.Height, 0));
        if (viewport != _viewport || extent != _extent)
        {
            _viewport = viewport;
            _extent = extent;
            ScrollOwner?.InvalidateScrollInfo();
        }

        // Clamping here rather than only in SetVerticalOffset: the listing can shrink under a
        // scrolled panel — a search narrowing, a delete — and an offset past the end would leave
        // the view blank with a scrollbar that says otherwise.
        var maxOffset = Math.Max(0, _extent.Height - _viewport.Height);
        if (_offset.Y > maxOffset)
        {
            _offset.Y = maxOffset;
            ScrollOwner?.InvalidateScrollInfo();
        }

        RealizeVisible(items.Count, availableSize);
        return new Size(
            double.IsInfinity(availableSize.Width) ? _extent.Width : availableSize.Width,
            double.IsInfinity(availableSize.Height) ? _extent.Height : availableSize.Height);
    }

    /// <summary>
    /// Works out which line every item is on, without building any of them.
    /// </summary>
    /// <remarks>
    /// The whole point of the panel, and the reason it is O(n) arithmetic: a media item is a tile of
    /// a known size and everything else is a line of its own, so the run of lines follows from the
    /// item list and the viewport width alone.
    /// </remarks>
    private void BuildLines(System.Collections.IList items, double viewportWidth)
    {
        _cell = _measuredCell ?? new Size(TileWidth + TileChromeX, TileHeight + TileChromeY);
        _rowHeight = _measuredRowHeight ?? DefaultRowHeight;
        _columns = Math.Max(1, (int)Math.Floor(Math.Max(viewportWidth, 1) / Math.Max(_cell.Width, 1)));

        _lines.Clear();
        if (_lineOfItem.Length < items.Count) _lineOfItem = new int[items.Count];

        var y = 0.0;
        var i = 0;
        while (i < items.Count)
        {
            if (!IsTile(items[i]))
            {
                _lineOfItem[i] = _lines.Count;
                _lines.Add(new Line(i, 1, y, _rowHeight, IsRow: true));
                y += _rowHeight;
                i++;
                continue;
            }

            // Consecutive tiles share a line, up to the column count. A row in the middle of them
            // breaks the line early, exactly as it does in a WrapPanel with a full-width child.
            var first = i;
            var count = 0;
            while (i < items.Count && count < _columns && IsTile(items[i]))
            {
                _lineOfItem[i] = _lines.Count;
                count++;
                i++;
            }
            _lines.Add(new Line(first, count, y, _cell.Height, IsRow: false));
            y += _cell.Height;
        }

        _extent = new Size(viewportWidth, y);
    }

    /// <summary>The same test the template selector makes, so the geometry and the template can
    /// never disagree about which shape an item is.</summary>
    private static bool IsTile(object? item) => ThumbnailTemplateSelector.IsTile(item);

    private void RealizeVisible(int itemCount, Size availableSize)
    {
        if (itemCount == 0 || _lines.Count == 0)
        {
            CleanUp(0, -1);
            _firstVisible = 0;
            _lastVisible = -1;
            return;
        }

        var top = _offset.Y;
        var bottom = _offset.Y + Math.Max(_viewport.Height, 1);

        var firstLine = FindLine(top);
        var lastLine = firstLine;
        while (lastLine + 1 < _lines.Count && _lines[lastLine + 1].Top < bottom) lastLine++;

        _firstVisible = _lines[firstLine].First;
        var last = _lines[lastLine];
        _lastVisible = Math.Min(itemCount - 1, last.First + last.Count - 1);

        var generator = ItemContainerGenerator;
        var start = generator.GeneratorPositionFromIndex(_firstVisible);
        // A realized item generates in place; an unrealized one is inserted after the previous
        // child, which is what Offset == 0 means.
        var childIndex = start.Offset == 0 ? start.Index : start.Index + 1;

        using (generator.StartAt(start, GeneratorDirection.Forward, true))
        {
            for (var i = _firstVisible; i <= _lastVisible; i++, childIndex++)
            {
                if (generator.GenerateNext(out var isNew) is not UIElement child) break;

                if (isNew)
                {
                    if (childIndex >= InternalChildren.Count) AddInternalChild(child);
                    else InsertInternalChild(childIndex, child);
                    generator.PrepareItemContainer(child);
                }

                var line = _lines[_lineOfItem[i]];
                child.Measure(line.IsRow
                    ? new Size(Math.Max(_viewport.Width, 0), double.PositiveInfinity)
                    : new Size(_cell.Width, _cell.Height));

                Correct(child, line.IsRow);
            }
        }

        CleanUp(_firstVisible, _lastVisible);
    }

    /// <summary>
    /// Believes the template over the constants above, once.
    /// </summary>
    /// <remarks>
    /// The assumed cell size mirrors margins that live in <c>Styles.xaml</c>, and a mirror can drift.
    /// Rather than let that show up as a quietly misaligned grid, the first tile that measures
    /// differently replaces the guess and buys one more layout pass. <see cref="_corrected"/> makes
    /// it once per size change, so a template that measures differently every time cannot loop.
    /// </remarks>
    private void Correct(UIElement child, bool isRow)
    {
        if (_corrected) return;
        var desired = child.DesiredSize;

        if (isRow)
        {
            if (desired.Height <= 0 || Math.Abs(desired.Height - _rowHeight) < 0.5) return;
            _measuredRowHeight = desired.Height;
        }
        else
        {
            if (desired.Width <= 0 || desired.Height <= 0) return;
            if (Math.Abs(desired.Width - _cell.Width) < 0.5 &&
                Math.Abs(desired.Height - _cell.Height) < 0.5) return;
            _measuredCell = desired;
        }

        _corrected = true;
        Dispatcher.BeginInvoke(DispatcherPriority.Loaded, InvalidateMeasure);
    }

    /// <summary>The first line whose bottom is past <paramref name="y"/> — a binary search, because
    /// a fast scroll over a large listing does this on every frame.</summary>
    private int FindLine(double y)
    {
        var lo = 0;
        var hi = _lines.Count - 1;
        while (lo < hi)
        {
            var mid = (lo + hi + 1) / 2;
            if (_lines[mid].Top <= y) lo = mid;
            else hi = mid - 1;
        }
        return lo;
    }

    /// <summary>Gives back the containers for everything that scrolled out of view.</summary>
    private void CleanUp(int first, int last)
    {
        var generator = ItemContainerGenerator;
        for (var i = InternalChildren.Count - 1; i >= 0; i--)
        {
            var position = new GeneratorPosition(i, 0);
            var itemIndex = generator.IndexFromGeneratorPosition(position);
            if (itemIndex >= first && itemIndex <= last) continue;

            generator.Remove(position, 1);
            RemoveInternalChildRange(i, 1);
        }
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        var generator = ItemContainerGenerator;
        for (var i = 0; i < InternalChildren.Count; i++)
        {
            var child = InternalChildren[i];
            var itemIndex = generator.IndexFromGeneratorPosition(new GeneratorPosition(i, 0));
            if (itemIndex < 0 || itemIndex >= _lineOfItem.Length) continue;

            var line = _lines[_lineOfItem[itemIndex]];
            var y = line.Top - _offset.Y;

            child.Arrange(line.IsRow
                ? new Rect(0, y, Math.Max(finalSize.Width, 0), line.Height)
                : new Rect((itemIndex - line.First) * _cell.Width, y, _cell.Width, line.Height));
        }
        return finalSize;
    }

    /// <summary>Rows removed under a scrolled panel take their containers with them; without this
    /// the panel keeps children for items that no longer exist.</summary>
    protected override void OnItemsChanged(object sender, ItemsChangedEventArgs args)
    {
        switch (args.Action)
        {
            case System.Collections.Specialized.NotifyCollectionChangedAction.Remove:
            case System.Collections.Specialized.NotifyCollectionChangedAction.Replace:
                RemoveInternalChildRange(args.Position.Index, args.ItemUICount);
                break;
            case System.Collections.Specialized.NotifyCollectionChangedAction.Reset:
                RemoveInternalChildRange(0, InternalChildren.Count);
                break;
        }
        InvalidateMeasure();
    }

    /// <summary>
    /// What <c>ScrollIntoView</c> reaches — type-ahead, revealing a file, restoring a selection.
    /// </summary>
    /// <remarks>
    /// A virtualized item has no element to bring into view, so the scrolling has to happen from the
    /// index alone and the container is generated by the layout pass that follows.
    /// </remarks>
    protected override void BringIndexIntoView(int index)
    {
        if (index < 0 || index >= _lineOfItem.Length || _lines.Count == 0) return;

        var line = _lines[_lineOfItem[index]];
        if (line.Top < _offset.Y) SetVerticalOffset(line.Top);
        else if (line.Top + line.Height > _offset.Y + _viewport.Height)
            SetVerticalOffset(line.Top + line.Height - _viewport.Height);
    }

    // --- IScrollInfo ---

    public bool CanVerticallyScroll { get; set; } = true;

    /// <summary>Always false: tiles wrap to the viewport, so there is nothing to the side of them.
    /// A full-width row is given the viewport's width for the same reason.</summary>
    public bool CanHorizontallyScroll
    {
        get => false;
        set { }
    }

    public double ExtentWidth => _extent.Width;
    public double ExtentHeight => _extent.Height;
    public double ViewportWidth => _viewport.Width;
    public double ViewportHeight => _viewport.Height;
    public double HorizontalOffset => 0;
    public double VerticalOffset => _offset.Y;
    public ScrollViewer? ScrollOwner { get; set; }

    /// <summary>
    /// One line's travel: a text row's worth, not a tile's.
    /// </summary>
    /// <remarks>
    /// Tied to the row height so the wheel covers about the same distance here as it does in the
    /// details list, where a line really is one row. A tile-height step would make the same notch
    /// move seven times further in this view than in the other one, which reads as the wheel being
    /// broken rather than as tiles being bigger.
    /// </remarks>
    private double LineStep => Math.Max(ScrollLineDelta, _rowHeight);

    public void LineUp() => SetVerticalOffset(_offset.Y - LineStep);
    public void LineDown() => SetVerticalOffset(_offset.Y + LineStep);
    public void MouseWheelUp() => SetVerticalOffset(_offset.Y - LineStep * 3);
    public void MouseWheelDown() => SetVerticalOffset(_offset.Y + LineStep * 3);
    public void PageUp() => SetVerticalOffset(_offset.Y - _viewport.Height);
    public void PageDown() => SetVerticalOffset(_offset.Y + _viewport.Height);

    public void LineLeft() { }
    public void LineRight() { }
    public void MouseWheelLeft() { }
    public void MouseWheelRight() { }
    public void PageLeft() { }
    public void PageRight() { }
    public void SetHorizontalOffset(double offset) { }

    public void SetVerticalOffset(double offset)
    {
        var clamped = Math.Max(0, Math.Min(offset, Math.Max(0, _extent.Height - _viewport.Height)));
        if (Math.Abs(clamped - _offset.Y) < 0.001) return;

        _offset.Y = clamped;
        ScrollOwner?.InvalidateScrollInfo();
        InvalidateMeasure();
    }

    /// <summary>Scrolls an element that is already realized into view — what clicking a partly
    /// visible tile, and keyboard focus moving onto one, both go through.</summary>
    public Rect MakeVisible(Visual visual, Rect rectangle)
    {
        if (visual is not UIElement child || !InternalChildren.Contains(child)) return rectangle;

        var top = child.TranslatePoint(new Point(0, 0), this).Y + _offset.Y;
        var bottom = top + child.RenderSize.Height;

        if (top < _offset.Y) SetVerticalOffset(top);
        else if (bottom > _offset.Y + _viewport.Height) SetVerticalOffset(bottom - _viewport.Height);

        return rectangle;
    }
}
