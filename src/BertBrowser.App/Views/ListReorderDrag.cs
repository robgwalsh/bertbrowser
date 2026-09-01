using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// Makes a <see cref="ListBox"/> reorderable by dragging its rows, with a line showing where the
/// row would land.
/// </summary>
/// <remarks>
/// <para>
/// Attached to the settings page's column list, which is what replaced its ↑ and ↓ buttons. The
/// drop reports an index and nothing else: the layout rules decide what that index means — a row
/// dropped above Name comes back second, because Name is not movable — so this class never needs to
/// know which list it is reordering.
/// </para>
/// <para>
/// <b>Mouse capture, not <c>DragDrop.DoDragDrop</c>.</b> A shell drag carries data between controls
/// and between processes, and it picks the place a drop may land by hit-testing for the nearest
/// element with <c>AllowDrop</c>. These rows hold a width <see cref="TextBox"/>, and
/// <see cref="TextBoxBase"/> switches <c>AllowDrop</c> on for itself so text can be dropped into
/// it — so the box, not the list, could win the drag and answer it the only way it can for a
/// payload that is not text: refused, which is the disallowed cursor. Nothing is leaving this list,
/// so there is no reason to involve the shell at all. Capturing the mouse keeps every event here
/// and makes the whole gesture this class's business.
/// </para>
/// </remarks>
internal sealed class ListReorderDrag
{
    /// <summary>How close to an edge the pointer must come before the list scrolls under it.</summary>
    private const double AutoScrollMargin = 18;

    private readonly ListBox _list;
    private readonly Action<int, int> _moved;
    private InsertionLine? _line;
    private Point _start;
    private int _from = -1;
    private bool _dragging;

    private ListReorderDrag(ListBox list, Action<int, int> moved)
    {
        _list = list;
        _moved = moved;

        list.PreviewMouseLeftButtonDown += OnPress;
        list.PreviewMouseMove += OnMove;
        list.PreviewMouseLeftButtonUp += OnRelease;
        list.LostMouseCapture += (_, _) => Cancel();
        list.PreviewKeyDown += OnKey;
    }

    /// <param name="moved">The row's old index and the index it was dropped at, in the list's own
    /// terms. Not called when the drop would not move anything.</param>
    public static void Attach(ListBox list, Action<int, int> moved) => _ = new ListReorderDrag(list, moved);

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
    internal static Adorner? ShowInsertionLine(ListBox list, int gap)
    {
        var drag = new ListReorderDrag(list, static (_, _) => { });
        drag.Show(gap);
        return drag._line;
    }

    private void OnPress(object sender, MouseButtonEventArgs e)
    {
        _from = -1;
        _start = e.GetPosition(_list);
        if (e.OriginalSource is not DependencyObject source) return;

        // A press that lands on a control inside the row belongs to that control: dragging across a
        // width box is selecting text in it, and pressing the × is pressing the ×.
        if (VisualTreeUtil.FindAncestor<TextBoxBase>(source) is not null) return;
        if (VisualTreeUtil.FindAncestor<ButtonBase>(source) is not null) return;

        if (VisualTreeUtil.FindAncestor<ListBoxItem>(source) is not { } row) return;
        _from = _list.ItemContainerGenerator.IndexFromContainer(row);
    }

    private void OnMove(object sender, MouseEventArgs e)
    {
        if (e.LeftButton != MouseButtonState.Pressed)
        {
            // The button came up somewhere this list never heard about — a menu, another window.
            if (_dragging) Cancel();
            return;
        }

        var now = e.GetPosition(_list);

        if (!_dragging)
        {
            if (_from < 0) return;

            // The system's own threshold, so a click that wobbles by a pixel is still a click. Below
            // it a drag would start on every selection and the list would be unusable on a trackpad.
            if (Math.Abs(now.X - _start.X) < SystemParameters.MinimumHorizontalDragDistance &&
                Math.Abs(now.Y - _start.Y) < SystemParameters.MinimumVerticalDragDistance)
                return;

            if (!_list.CaptureMouse()) return;
            _dragging = true;
            _list.Cursor = Cursors.SizeNS;
        }

        AutoScroll(now);
        Show(Target(now));
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
        var target = Target(e.GetPosition(_list));
        Cancel();
        e.Handled = true;
        if (from < 0) return;

        // A row released just below itself is where it already is: the gap under row 3 and the gap
        // above row 3 are both "row 3 stays put", and moving it would look like a bug.
        if (target > from) target--;
        if (target != from) _moved(from, target);
    }

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
        _list.ClearValue(FrameworkElement.CursorProperty);
        if (!_dragging) return;

        _dragging = false;
        if (_list.IsMouseCaptured) _list.ReleaseMouseCapture();
    }

    /// <summary>Scrolls when the pointer reaches an edge, so a list longer than its box can be
    /// reordered end to end without letting go.</summary>
    private void AutoScroll(Point point)
    {
        if (VisualTreeUtil.FindDescendant<ScrollViewer>(_list) is not { } scroller) return;

        if (point.Y < AutoScrollMargin)
            scroller.ScrollToVerticalOffset(scroller.VerticalOffset - 1);
        else if (point.Y > _list.ActualHeight - AutoScrollMargin)
            scroller.ScrollToVerticalOffset(scroller.VerticalOffset + 1);
    }

    /// <summary>
    /// The gap the row would land in: 0 is above the first row, Count is below the last.
    /// </summary>
    /// <remarks>
    /// Measured against each row's midpoint rather than its top, so the line flips to the far side
    /// halfway down a row — the behaviour every reorderable list has, and the reason a drop feels
    /// aimed rather than approximate.
    /// </remarks>
    private int Target(Point point)
    {
        for (var i = 0; i < _list.Items.Count; i++)
        {
            if (Row(i) is not { } row) continue;
            var top = row.TranslatePoint(new Point(0, 0), _list).Y;
            if (point.Y < top + (row.ActualHeight / 2)) return i;
        }
        return _list.Items.Count;
    }

    /// <summary>Where the line is drawn for a gap: the top of that row, or the bottom of the last
    /// one for the gap past the end.</summary>
    private double LineY(int target)
    {
        if (Row(Math.Min(target, _list.Items.Count - 1)) is not { } row) return 0;
        var top = row.TranslatePoint(new Point(0, 0), _list).Y;
        return target >= _list.Items.Count ? top + row.ActualHeight : top;
    }

    private void Show(int target)
    {
        if (_list.Items.Count == 0) return;
        if (_line is null)
        {
            if (AdornerLayer.GetAdornerLayer(_list) is not { } layer) return;
            _line = new InsertionLine(_list);
            layer.Add(_line);
        }
        _line.MoveTo(LineY(target));
    }

    private void Hide()
    {
        if (_line is null) return;
        AdornerLayer.GetAdornerLayer(_list)?.Remove(_line);
        _line = null;
    }

    private ListBoxItem? Row(int index) =>
        _list.ItemContainerGenerator.ContainerFromIndex(index) as ListBoxItem;

    /// <summary>The line between two rows. An adorner rather than a child of the list, so it draws
    /// over the rows without the list having to leave a gap for it.</summary>
    private sealed class InsertionLine : Adorner
    {
        private readonly Pen _pen;
        private double _y = double.NaN;

        /// <remarks>
        /// The pen is <b>deliberately not frozen</b>, for the two reasons <c>MarqueeSelector</c>
        /// gives: it holds the shared theme brush instance, so recolouring on a theme change
        /// repaints the line for free — and freezing it does not merely pin the colour, it throws,
        /// because a live theme brush is not freezable and neither is a pen holding one. Missing the
        /// resource falls back rather than throwing, so a list adorned outside the themed app still
        /// draws a line.
        /// </remarks>
        public InsertionLine(UIElement list) : base(list)
        {
            IsHitTestVisible = false;
            var brush = Application.Current?.TryFindResource(ThemeToken.AccentBackground) as Brush
                ?? new SolidColorBrush(Color.FromRgb(0x00, 0x7A, 0xCC));
            _pen = new Pen(brush, 2);
        }

        public void MoveTo(double y)
        {
            if (Math.Abs(y - _y) < 0.5) return; // NaN compares false, so the first move always draws.
            _y = y;
            InvalidateVisual();
        }

        protected override void OnRender(DrawingContext context)
        {
            var width = ((FrameworkElement)AdornedElement).ActualWidth;
            context.DrawLine(_pen, new Point(1, _y), new Point(width - 1, _y));
        }
    }
}
