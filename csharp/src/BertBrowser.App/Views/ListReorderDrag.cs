using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// Makes an <see cref="ItemsControl"/> reorderable by dragging its items, with a line showing
/// where the item would land. Vertical for a list of rows, horizontal for a strip of tabs.
/// </summary>
/// <remarks>
/// <para>
/// Attached to the settings page's column list, which is what replaced its ↑ and ↓ buttons, to
/// each pane's tab strip, and to the Context menu page's preview — which also takes items carried
/// in from the lists beside it (<see cref="AttachInsert"/>). The drop reports an index and nothing else: the caller decides what that
/// index means — a row dropped above Name comes back second, because Name is not movable — so this
/// class never needs to know which list it is reordering. It also never needs to know what kind of
/// container the list generates: a <see cref="ListBox"/> wraps items in a <c>ListBoxItem</c> and a
/// plain <see cref="ItemsControl"/> in a <c>ContentPresenter</c>, and both are found through the
/// control's own generator.
/// </para>
/// <para>
/// <b>Mouse capture, not <c>DragDrop.DoDragDrop</c>.</b> A shell drag carries data between controls
/// and between processes, and it picks the place a drop may land by hit-testing for the nearest
/// element with <c>AllowDrop</c>. The column rows hold a width <see cref="TextBox"/>, and
/// <see cref="TextBoxBase"/> switches <c>AllowDrop</c> on for itself so text can be dropped into
/// it — so the box, not the list, could win the drag and answer it the only way it can for a
/// payload that is not text: refused, which is the disallowed cursor. The tab strip has the
/// opposite problem: the file list under it accepts shell drags of files, and a tab is not a file.
/// Nothing is leaving either list, so there is no reason to involve the shell at all. Capturing the
/// mouse keeps every event here and makes the whole gesture this class's business.
/// </para>
/// </remarks>
internal sealed class ListReorderDrag
{
    /// <summary>How close to an edge the pointer must come before the list scrolls under it.</summary>
    private const double AutoScrollMargin = 18;

    /// <summary>The list the line is drawn in and a drop lands in.</summary>
    private readonly ItemsControl _list;

    /// <summary>Where a drag starts: <see cref="_list"/> itself when reordering, another list when
    /// carrying an item across into it.</summary>
    private readonly ItemsControl _source;

    private readonly Orientation _orientation;
    private readonly Action<int, int>? _moved;
    private readonly Action<object, int>? _inserted;
    private object? _fromItem;
    private InsertionLine? _line;
    private Point _start;
    private int _from = -1;
    private bool _dragging;

    private ListReorderDrag(
        ItemsControl source, ItemsControl list, Orientation orientation,
        Action<int, int>? moved, Action<object, int>? inserted)
    {
        _source = source;
        _list = list;
        _orientation = orientation;
        _moved = moved;
        _inserted = inserted;

        source.PreviewMouseLeftButtonDown += OnPress;
        source.PreviewMouseMove += OnMove;
        source.PreviewMouseLeftButtonUp += OnRelease;
        source.LostMouseCapture += (_, _) => Cancel();
        source.PreviewKeyDown += OnKey;
    }

    /// <param name="orientation">Which way the items are laid out, i.e. which way a drag moves.</param>
    /// <param name="moved">The item's old index and the index it was dropped at, in the list's own
    /// terms. Not called when the drop would not move anything.</param>
    public static void Attach(ItemsControl list, Orientation orientation, Action<int, int> moved) =>
        _ = new ListReorderDrag(list, list, orientation, moved, null);

    /// <summary>
    /// Lets items be dragged out of <paramref name="source"/> and dropped into a gap of
    /// <paramref name="target"/>, with the same line showing where. Nothing leaves the source: what a
    /// drop means is the caller's business, as it is for a reorder.
    /// </summary>
    /// <param name="inserted">The source item and the target gap it was dropped in (0 is before
    /// the first item). Not called for a drop outside the target.</param>
    public static void AttachInsert(
        ItemsControl source, ItemsControl target, Orientation orientation, Action<object, int> inserted) =>
        _ = new ListReorderDrag(source, target, orientation, null, inserted);

    /// <summary>
    /// Draws the insertion line for a gap and leaves it there, for the harness.
    /// </summary>
    /// <remarks>
    /// The line is only ever on screen in the middle of a real mouse drag, and a harness run posts
    /// no mouse input, so this is the only way a capture can reach it. It also earns its keep as a
    /// regression test: building the line is what once took the whole app down — the pen held a
    /// live theme brush, which cannot be frozen — so a run that gets here at all has proved the
    /// adorner still constructs and renders against real theme resources.
    /// </remarks>
    /// <returns>The line, or null if the list has no adorner layer to draw on.</returns>
    internal static Adorner? ShowInsertionLine(ItemsControl list, Orientation orientation, int gap)
    {
        var drag = new ListReorderDrag(list, list, orientation, static (_, _) => { }, null);
        drag.Show(gap);
        return drag._line;
    }

    private void OnPress(object sender, MouseButtonEventArgs e)
    {
        _from = -1;
        _fromItem = null;
        _start = e.GetPosition(_source);
        if (e.OriginalSource is not DependencyObject source) return;

        // A press that lands on a control inside the item belongs to that control: dragging across
        // a width box is selecting text in it, and pressing the × is pressing the ×.
        if (VisualTreeUtil.FindAncestor<TextBoxBase>(source) is not null) return;
        if (VisualTreeUtil.FindAncestor<ButtonBase>(source) is not null) return;

        if (ItemsControl.ContainerFromElement(_source, source) is not { } container) return;
        _from = _source.ItemContainerGenerator.IndexFromContainer(container);
        _fromItem = _source.ItemContainerGenerator.ItemFromContainer(container);
    }

    private void OnMove(object sender, MouseEventArgs e)
    {
        if (e.LeftButton != MouseButtonState.Pressed)
        {
            // The button came up somewhere this list never heard about — a menu, another window.
            if (_dragging) Cancel();
            return;
        }

        if (!_dragging)
        {
            if (_from < 0) return;

            // The system's own threshold, so a click that wobbles by a pixel is still a click. Below
            // it a drag would start on every selection and the list would be unusable on a trackpad.
            var moved = e.GetPosition(_source);
            if (Math.Abs(moved.X - _start.X) < SystemParameters.MinimumHorizontalDragDistance &&
                Math.Abs(moved.Y - _start.Y) < SystemParameters.MinimumVerticalDragDistance)
                return;

            if (!_source.CaptureMouse()) return;
            _dragging = true;
            _source.Cursor = _inserted is not null ? Cursors.Hand
                : _orientation == Orientation.Vertical ? Cursors.SizeNS : Cursors.SizeWE;
        }

        // Carried across, the line shows only while the pointer is over the list it would land in.
        var now = e.GetPosition(_list);
        if (_inserted is not null && !IsOver(now))
        {
            Hide();
        }
        else
        {
            AutoScroll(e);
            Show(Target(now));
        }
        e.Handled = true;
    }

    private void OnRelease(object sender, MouseButtonEventArgs e)
    {
        if (!_dragging)
        {
            _from = -1;
            return;
        }

        var from = _from;
        var item = _fromItem;
        var at = e.GetPosition(_list);
        var target = Target(at);
        Cancel();
        e.Handled = true;
        if (from < 0) return;

        if (_inserted is not null)
        {
            if (item is not null && IsOver(at)) _inserted(item, target);
            return;
        }

        // An item released just past itself is where it already is: the gap after item 3 and the
        // gap before item 3 are both "item 3 stays put", and moving it would look like a bug.
        if (target > from) target--;
        if (target != from) _moved?.Invoke(from, target);
    }

    private bool IsOver(Point point) =>
        point.X >= 0 && point.Y >= 0 && point.X <= _list.ActualWidth && point.Y <= _list.ActualHeight;

    private void OnKey(object sender, KeyEventArgs e)
    {
        if (!_dragging || e.Key != Key.Escape) return;
        Cancel();
        e.Handled = true;
    }

    /// <summary>Ends the gesture and puts back everything it touched, whether it was committed,
    /// abandoned or interrupted. Safe to call when nothing is happening.</summary>
    private void Cancel()
    {
        Hide();
        _from = -1;
        _fromItem = null;
        _source.ClearValue(FrameworkElement.CursorProperty);
        if (!_dragging) return;

        _dragging = false;
        if (_source.IsMouseCaptured) _source.ReleaseMouseCapture();
    }

    /// <summary>Scrolls when the pointer reaches an edge, so a list longer than its box can be
    /// reordered end to end without letting go.</summary>
    /// <remarks>A <see cref="ListBox"/> scrolls itself, so its scroller is inside it; the tab strip
    /// sits inside a scroller of the pane's, so that one is found upward. The edges are the
    /// scroller's either way — the strip itself is as wide as every tab together.</remarks>
    private void AutoScroll(MouseEventArgs e)
    {
        var scroller = VisualTreeUtil.FindDescendant<ScrollViewer>(_list)
            ?? VisualTreeUtil.FindAncestor<ScrollViewer>(VisualTreeUtil.ParentOf(_list));
        if (scroller is null) return;

        var at = Along(e.GetPosition(scroller));
        var extent = Extent(scroller);
        if (at < AutoScrollMargin)
            ScrollBy(scroller, -1);
        else if (at > extent - AutoScrollMargin)
            ScrollBy(scroller, +1);
    }

    private void ScrollBy(ScrollViewer scroller, double delta)
    {
        if (_orientation == Orientation.Vertical)
            scroller.ScrollToVerticalOffset(scroller.VerticalOffset + delta);
        else
            scroller.ScrollToHorizontalOffset(scroller.HorizontalOffset + delta);
    }

    /// <summary>
    /// The gap the item would land in: 0 is before the first item, Count is after the last.
    /// </summary>
    /// <remarks>
    /// Measured against each item's midpoint rather than its leading edge, so the line flips to the
    /// far side halfway along an item — the behaviour every reorderable list has, and the reason a
    /// drop feels aimed rather than approximate.
    /// </remarks>
    private int Target(Point point)
    {
        for (var i = 0; i < _list.Items.Count; i++)
        {
            if (Item(i) is not { } item) continue;
            if (Along(point) < Start(item) + (Extent(item) / 2)) return i;
        }
        return _list.Items.Count;
    }

    /// <summary>Where the line is drawn for a gap: the leading edge of that item, or the trailing
    /// edge of the last one for the gap past the end.</summary>
    private double LineAt(int target)
    {
        if (Item(Math.Min(target, _list.Items.Count - 1)) is not { } item) return 0;
        var start = Start(item);
        return target >= _list.Items.Count ? start + Extent(item) : start;
    }

    private void Show(int target)
    {
        if (_list.Items.Count == 0) return;
        if (_line is null)
        {
            if (AdornerLayer.GetAdornerLayer(_list) is not { } layer) return;
            _line = new InsertionLine(_list, _orientation);
            layer.Add(_line);
        }
        _line.MoveTo(LineAt(target));
    }

    private void Hide()
    {
        if (_line is null) return;
        AdornerLayer.GetAdornerLayer(_list)?.Remove(_line);
        _line = null;
    }

    private FrameworkElement? Item(int index) =>
        _list.ItemContainerGenerator.ContainerFromIndex(index) as FrameworkElement;

    // The one axis a drag moves along. Everything above is written against these three so that the
    // vertical and horizontal cases cannot drift apart.

    private double Along(Point point) => _orientation == Orientation.Vertical ? point.Y : point.X;

    private double Extent(FrameworkElement element) =>
        _orientation == Orientation.Vertical ? element.ActualHeight : element.ActualWidth;

    private double Start(FrameworkElement item) => Along(item.TranslatePoint(new Point(0, 0), _list));

    /// <summary>The line between two items. An adorner rather than a child of the list, so it draws
    /// over the items without the list having to leave a gap for it.</summary>
    private sealed class InsertionLine : Adorner
    {
        private readonly Pen _pen;
        private readonly Orientation _orientation;
        private double _at = double.NaN;

        /// <remarks>
        /// The pen is <b>deliberately not frozen</b>, for the two reasons <c>MarqueeSelector</c>
        /// gives: it holds the shared theme brush instance, so recolouring on a theme change
        /// repaints the line for free — and freezing it does not merely pin the colour, it throws,
        /// because a live theme brush is not freezable and neither is a pen holding one. Missing the
        /// resource falls back rather than throwing, so a list adorned outside the themed app still
        /// draws a line.
        /// </remarks>
        public InsertionLine(UIElement list, Orientation orientation) : base(list)
        {
            IsHitTestVisible = false;
            _orientation = orientation;
            var brush = Application.Current?.TryFindResource(ThemeToken.AccentBackground) as Brush
                ?? new SolidColorBrush(Color.FromRgb(0x00, 0x7A, 0xCC));
            _pen = new Pen(brush, 2);
        }

        public void MoveTo(double at)
        {
            if (Math.Abs(at - _at) < 0.5) return; // NaN compares false, so the first move always draws.
            _at = at;
            InvalidateVisual();
        }

        protected override void OnRender(DrawingContext context)
        {
            var element = (FrameworkElement)AdornedElement;
            if (_orientation == Orientation.Vertical)
                context.DrawLine(_pen, new Point(1, _at), new Point(element.ActualWidth - 1, _at));
            else
                context.DrawLine(_pen, new Point(_at, 1), new Point(_at, element.ActualHeight - 1));
        }
    }
}
