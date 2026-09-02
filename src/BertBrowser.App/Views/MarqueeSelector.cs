using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Media3D;
using System.Windows.Threading;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// Explorer-style rubber-band selection for a <see cref="ListView"/>: pressing on empty space
/// (below the rows in details mode, or in the gaps between thumbnail tiles) and dragging sweeps
/// out a rectangle that selects everything it touches. Holding Ctrl or Shift keeps whatever was
/// already selected when the drag started; a plain drag replaces the selection, and a plain click
/// on empty space clears it. Dragging past the top or bottom edge auto-scrolls the list.
/// </summary>
internal sealed class MarqueeSelector
{
    private const double AutoScrollMargin = 20;

    /// <summary>How far past the viewport the anchor is placed once it has scrolled out of the
    /// realized range — far enough that the rectangle still covers every row on that side.</summary>
    private const double OffscreenSpan = 100_000;

    private readonly ListView _list;
    private readonly DispatcherTimer _autoScroll;

    private bool _pending;   // pressed on empty space, drag threshold not yet crossed
    private bool _additive;  // Ctrl/Shift was down at mouse-down: keep the prior selection
    private Point _origin;   // mouse-down point, in list coordinates
    private Point _cursor;   // latest cursor position, in list coordinates

    // The drag anchor is pinned to the item it landed on, so the rectangle keeps covering the same
    // content while the list scrolls (and while rows virtualize in and out) underneath it.
    private int _anchorIndex = -1;
    private Vector _anchorOffset;
    private double _originHorizontalOffset;

    private readonly HashSet<object> _hits = new();
    private object[] _initialSelection = Array.Empty<object>();

    private Panel? _itemsHost;
    private ScrollViewer? _scroller;
    private ScrollContentPresenter? _viewport;
    private MarqueeAdorner? _adorner;

    /// <summary>True while a rubber-band drag is in progress. The view suppresses per-selection
    /// side effects (folder-tree reveal) while it is set, since selection churns every frame.</summary>
    public bool IsDragging { get; private set; }

    /// <summary>Raised once the rubber band is released. Work that is skipped during a drag —
    /// the preview pane's read, most of all — has to be told when the drag is over, because the
    /// last selection change of a sweep happens while the band is still down and nothing else
    /// fires afterwards.</summary>
    public event Action? DragEnded;

    public static MarqueeSelector Attach(ListView list) => new(list);

    /// <summary>Draws the band over <paramref name="rect"/> (in list coordinates) and leaves it
    /// there, for the harness — mirrors <see cref="ListReorderDrag.ShowInsertionLine"/>: a run
    /// posts no mouse input, so this is the only way a capture can reach it. It also earns its keep
    /// as a regression test: reaching a screenshot at all proves the adorner layer is still found
    /// and the theme brushes still resolve.</summary>
    /// <returns>The adorner, or null if the list has no adorner layer to draw on.</returns>
    internal static Adorner? ShowBand(ListView list, Rect rect)
    {
        var selector = new MarqueeSelector(list) { _origin = rect.TopLeft, _cursor = rect.BottomRight };
        selector.BeginDrag();
        selector.Update();
        return selector._adorner;
    }

    private MarqueeSelector(ListView list)
    {
        _list = list;
        _autoScroll = new DispatcherTimer(DispatcherPriority.Input) { Interval = TimeSpan.FromMilliseconds(50) };
        _autoScroll.Tick += (_, _) => AutoScroll();

        list.PreviewMouseLeftButtonDown += OnMouseDown;
        list.PreviewMouseMove += OnMouseMove;
        list.PreviewMouseLeftButtonUp += OnMouseUp;
        list.LostMouseCapture += (_, _) =>
        {
            if (IsDragging) EndDrag();
        };
    }

    // --- Mouse handling ---

    private void OnMouseDown(object sender, MouseButtonEventArgs e)
    {
        _pending = false;
        var point = e.GetPosition(_list);
        var empty = IsEmptySpace(point, e.OriginalSource as DependencyObject);
        if (e.ClickCount > 1 || !empty) return;

        _origin = _cursor = point;
        _additive = (Keyboard.Modifiers & (ModifierKeys.Control | ModifierKeys.Shift)) != 0;
        _pending = true;
        // Deliberately not handled: a press that never turns into a drag must still focus the list.
    }

    private void OnMouseMove(object sender, MouseEventArgs e)
    {
        if (!_pending) return;
        if (e.LeftButton != MouseButtonState.Pressed)
        {
            if (IsDragging) EndDrag(); else _pending = false;
            return;
        }

        _cursor = e.GetPosition(_list);
        if (!IsDragging)
        {
            if (Math.Abs(_cursor.X - _origin.X) < SystemParameters.MinimumHorizontalDragDistance &&
                Math.Abs(_cursor.Y - _origin.Y) < SystemParameters.MinimumVerticalDragDistance)
                return;
            BeginDrag();
        }

        Update();
        e.Handled = true;
    }

    private void OnMouseUp(object sender, MouseButtonEventArgs e)
    {
        if (IsDragging)
        {
            EndDrag();
            e.Handled = true;
            return;
        }
        if (!_pending) return;
        _pending = false;

        // A plain click on empty space drops the selection, like Explorer.
        if (!_additive && _list.SelectedItems.Count > 0)
            _list.UnselectAll();
    }

    // --- Drag lifecycle ---

    private void BeginDrag()
    {
        IsDragging = true;
        _itemsHost = FindItemsHost(_list);
        // A GridView's ScrollViewer wraps its header row in a ScrollContentPresenter of its own
        // (so the header tracks horizontal scroll without scrolling vertically) — a top-down search
        // from the list finds that one first, not the one actually hosting the rows. Walking up
        // from the items host instead can't land on the wrong one.
        _viewport = _itemsHost is null ? null : VisualTreeUtil.FindAncestor<ScrollContentPresenter>(_itemsHost);
        _scroller = _viewport is null ? null : VisualTreeUtil.FindAncestor<ScrollViewer>(_viewport);
        _originHorizontalOffset = _scroller?.HorizontalOffset ?? 0;
        _initialSelection = _list.SelectedItems.Cast<object>().ToArray();
        _hits.Clear();
        CaptureAnchor();

        _list.CaptureMouse();
        var layer = AdornerLayer.GetAdornerLayer(_list);
        if (layer is { } l)
        {
            _adorner = new MarqueeAdorner(_list);
            l.Add(_adorner);
        }
        _autoScroll.Start();
    }

    private void EndDrag()
    {
        IsDragging = false;
        _pending = false;
        _autoScroll.Stop();

        if (_adorner is { } adorner)
        {
            AdornerLayer.GetAdornerLayer(_list)?.Remove(adorner);
            _adorner = null;
        }
        if (_list.IsMouseCaptured)
            _list.ReleaseMouseCapture();

        _hits.Clear();
        _initialSelection = Array.Empty<object>();
        _anchorIndex = -1;
        _itemsHost = null;
        _scroller = null;
        _viewport = null;

        DragEnded?.Invoke();
    }

    /// <summary>Pins the drag origin to the realized row nearest it, remembering the offset within
    /// that row so the anchor can be recomputed after the list scrolls.</summary>
    private void CaptureAnchor()
    {
        _anchorIndex = -1;
        var best = double.MaxValue;
        foreach (var (_, index, bounds) in RealizedItems())
        {
            var distance = DistanceTo(bounds, _origin);
            if (distance >= best) continue;
            best = distance;
            _anchorIndex = index;
            _anchorOffset = _origin - bounds.TopLeft;
        }
    }

    private static double DistanceTo(Rect rect, Point point)
    {
        var dx = Math.Max(0, Math.Max(rect.Left - point.X, point.X - rect.Right));
        var dy = Math.Max(0, Math.Max(rect.Top - point.Y, point.Y - rect.Bottom));
        return dx * dx + dy * dy;
    }

    // --- Per-frame update ---

    private void Update()
    {
        var realized = RealizedItems().ToList();
        var rect = new Rect(CurrentAnchor(realized), _cursor);

        foreach (var (container, _, bounds) in realized)
        {
            var item = _list.ItemContainerGenerator.ItemFromContainer(container);
            if (item == DependencyProperty.UnsetValue) continue;
            if (rect.IntersectsWith(bounds)) _hits.Add(item);
            else _hits.Remove(item);
        }

        SyncSelection();
        var clipped = Rect.Intersect(rect, ViewportRect());
        _adorner?.SetRect(clipped);
    }

    /// <summary>The drag origin in current list coordinates: tracked through the anchor row while
    /// it stays realized, and pushed off the near edge once it has scrolled out of range.</summary>
    private Point CurrentAnchor(List<(ListViewItem Container, int Index, Rect Bounds)> realized)
    {
        var x = _origin.X - ((_scroller?.HorizontalOffset ?? 0) - _originHorizontalOffset);
        if (_anchorIndex < 0) return new Point(x, _origin.Y);

        if (_list.ItemContainerGenerator.ContainerFromIndex(_anchorIndex) is ListViewItem anchor &&
            TryGetBounds(anchor, out var bounds))
            return bounds.TopLeft + _anchorOffset;

        // Virtualized away: everything realized is on one side of the anchor.
        var scrolledPastTop = realized.Count == 0 || _anchorIndex < realized.Min(r => r.Index);
        return new Point(x, scrolledPastTop ? -OffscreenSpan : _list.ActualHeight + OffscreenSpan);
    }

    /// <summary>Pushes the accumulated hits (plus the pre-drag selection when additive) onto the
    /// list, touching only the items that actually changed so selection churn stays minimal.</summary>
    private void SyncSelection()
    {
        var target = _additive ? new HashSet<object>(_initialSelection) : new HashSet<object>();
        target.UnionWith(_hits);

        var selected = _list.SelectedItems;
        var current = new HashSet<object>(selected.Cast<object>());

        for (var i = selected.Count - 1; i >= 0; i--)
        {
            if (selected[i] is { } item && !target.Contains(item))
                selected.RemoveAt(i);
        }
        foreach (var item in target)
        {
            if (!current.Contains(item))
                selected.Add(item);
        }
    }

    private void AutoScroll()
    {
        if (!IsDragging) return;
        _cursor = Mouse.GetPosition(_list);

        if (_scroller is { } scroller)
        {
            var overshoot = _cursor.Y < AutoScrollMargin
                ? _cursor.Y - AutoScrollMargin
                : _cursor.Y > _list.ActualHeight - AutoScrollMargin
                    ? _cursor.Y - (_list.ActualHeight - AutoScrollMargin)
                    : 0;
            if (overshoot != 0)
            {
                var lines = Math.Clamp((int)(Math.Abs(overshoot) / 12), 1, 6);
                for (var i = 0; i < lines; i++)
                {
                    if (overshoot < 0) scroller.LineUp();
                    else scroller.LineDown();
                }
                // Realize the rows that just scrolled in before hit-testing against them.
                scroller.UpdateLayout();
            }
        }

        Update();
    }

    // --- Visual tree helpers ---

    private IEnumerable<(ListViewItem Container, int Index, Rect Bounds)> RealizedItems()
    {
        if (_itemsHost is null) yield break;
        foreach (var child in _itemsHost.Children)
        {
            if (child is not ListViewItem container || !TryGetBounds(container, out var bounds)) continue;
            // A container the generator doesn't know yet has no index to anchor or order by.
            var index = _list.ItemContainerGenerator.IndexFromContainer(container);
            if (index >= 0) yield return (container, index, bounds);
        }
    }

    private bool TryGetBounds(ListViewItem container, out Rect bounds)
    {
        bounds = Rect.Empty;
        if (container.Visibility != Visibility.Visible) return false;
        var size = container.RenderSize;
        if (size.Width <= 0 || size.Height <= 0) return false;
        try
        {
            bounds = new Rect(container.TransformToAncestor(_list).Transform(default), size);
            return true;
        }
        catch (InvalidOperationException)
        {
            return false; // not connected to the list's visual tree yet
        }
    }

    /// <summary>The scrolled content area in list coordinates, so the drawn rectangle never spills
    /// over the column headers or the scroll bars.</summary>
    private Rect ViewportRect()
    {
        var full = new Rect(0, 0, _list.ActualWidth, _list.ActualHeight);
        if (_viewport is null) return full;
        try
        {
            var origin = _viewport.TransformToAncestor(_list).Transform(default);
            return Rect.Intersect(full, new Rect(origin, _viewport.RenderSize));
        }
        catch (InvalidOperationException)
        {
            return full;
        }
    }

    /// <summary>True when the press landed on the list's background rather than on a row's actual
    /// content, a column header, or a scroll bar. In details mode a row's container is stretched to
    /// the full width of the list — its background is what a click past the last column actually
    /// hits — so that strip (there whenever the columns don't fill the pane) counts as empty space
    /// too, exactly like the gap below the last row, or Explorer would refuse to start a band from
    /// almost anywhere in a populated list.</summary>
    private bool IsEmptySpace(Point point, DependencyObject? source)
    {
        for (var d = source; d is not null; d = VisualTreeUtil.ParentOf(d))
        {
            if (d is GridViewColumnHeader or ScrollBar or Thumb) return false;
            if (d is ListViewItem) return _list.View is GridView grid && point.X >= TotalColumnsWidth(grid);
            if (ReferenceEquals(d, _list)) break;
        }
        return true;
    }

    private static double TotalColumnsWidth(GridView grid) => grid.Columns.Sum(c => c.ActualWidth);

    private static Panel? FindItemsHost(DependencyObject root)
    {
        if (root is Panel { IsItemsHost: true } panel) return panel;
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            if (FindItemsHost(VisualTreeHelper.GetChild(root, i)) is { } found) return found;
        }
        return null;
    }

    /// <summary>Draws the selection rectangle above the list without disturbing its layout.</summary>
    private sealed class MarqueeAdorner : Adorner
    {
        // Taken from the app palette rather than baked in, and deliberately not frozen: these are
        // the shared theme brush instances, so recolouring them on a theme change repaints the
        // band for free. Freezing — which the previous version did — would pin it to one theme.
        private readonly Brush _fill;
        private readonly Pen _border;

        private Rect _rect;

        public MarqueeAdorner(UIElement adorned) : base(adorned)
        {
            IsHitTestVisible = false;
            _fill = Resolve(ThemeToken.MarqueeFill, Color.FromArgb(0x33, 0x00, 0x7A, 0xCC));
            _border = new Pen(Resolve(ThemeToken.MarqueeBorder, Color.FromRgb(0x00, 0x7A, 0xCC)), 1);
        }

        private static Brush Resolve(string token, Color fallback) =>
            Application.Current?.TryFindResource(token) as Brush ?? new SolidColorBrush(fallback);

        public void SetRect(Rect rect)
        {
            if (rect == _rect) return;
            _rect = rect;
            InvalidateVisual();
        }

        protected override void OnRender(DrawingContext drawingContext)
        {
            if (_rect.IsEmpty || _rect.Width <= 0 || _rect.Height <= 0) return;
            drawingContext.DrawRectangle(_fill, _border, _rect);
        }
    }
}
