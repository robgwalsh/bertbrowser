using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;

namespace BertBrowser.App.Views;

/// <summary>
/// Renders the pane layout tree as nested <see cref="Grid"/>s with real
/// <see cref="GridSplitter"/>s between the slots.
/// </summary>
/// <remarks>
/// Built in code rather than by a recursive template because WPF cannot bind
/// <see cref="Grid.ColumnDefinitions"/>, and the splitters are not items of any collection — a
/// declarative version would have to re-implement <see cref="GridSplitter"/> inside a custom panel.
/// <para>
/// Rebuilds happen only when the arrangement itself changes (a split or a close), never on
/// navigation, and each pane's view is <em>re-parented</em> rather than recreated, so a pane that
/// wasn't touched keeps its tabs, selections and realized rows.
/// </para>
/// </remarks>
internal sealed class PaneLayoutHost : ContentControl
{
    /// <summary>Splitter thickness in device-independent pixels. A fixed-pixel slot rather than
    /// Auto, so it is subtracted from the available space before the star weights divide it.</summary>
    private const double SplitterThickness = 6;

    private const double MinPaneSize = 160;

    private readonly ShellViewModel _shell;
    private readonly AppSettings _settings;
    private readonly Dictionary<PaneViewModel, FilePaneView> _views = new();

    public PaneLayoutHost(ShellViewModel shell, AppSettings settings)
    {
        _shell = shell;
        _settings = settings;
        // ContentControl defaults to Left/Top, which would leave the pane grid sized to its content
        // instead of filling the window.
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        VerticalContentAlignment = VerticalAlignment.Stretch;

        _shell.LayoutChanged += Rebuild;
        Rebuild();
    }

    public FilePaneView? ViewFor(PaneViewModel pane) =>
        _views.TryGetValue(pane, out var view) ? view : null;

    public FilePaneView? ActivePaneView => ViewFor(_shell.ActivePane);

    public void Rebuild()
    {
        var live = LayoutTree.Leaves(_shell.Layout).Select(l => l.Value).ToHashSet();

        // Re-parenting resets the ListView's scroll offset, so put every pane back where it was.
        var offsets = CaptureScrollOffsets(live);

        Content = null;
        Content = BuildNode(_shell.Layout);

        foreach (var pane in _views.Keys.Where(p => !live.Contains(p)).ToList())
        {
            _views[pane].Detach();
            _views.Remove(pane);
        }

        foreach (var view in _views.Values)
            view.UpdateClosePaneAvailability();

        _ = Dispatcher.InvokeAsync(() => RestoreScrollOffsets(offsets), DispatcherPriority.Loaded);
    }

    private UIElement BuildNode(ILayoutNode<PaneViewModel> node) => node switch
    {
        LayoutLeaf<PaneViewModel> leaf => LeafView(leaf.Value),
        LayoutSplit<PaneViewModel> split => BuildSplit(split),
        _ => new Grid(),
    };

    private FilePaneView LeafView(PaneViewModel pane)
    {
        if (!_views.TryGetValue(pane, out var view))
        {
            view = new FilePaneView(_shell, _settings, pane);
            _views[pane] = view;
        }
        // The same instance moves between grids across rebuilds; WPF refuses a second parent.
        (view.Parent as Panel)?.Children.Remove(view);
        return view;
    }

    private Grid BuildSplit(LayoutSplit<PaneViewModel> split)
    {
        var grid = new Grid();
        var vertical = split.Orientation == SplitOrientation.Vertical;

        // 2n-1 slots: a star-sized slot per child, with a fixed splitter slot between each pair.
        for (var i = 0; i < split.Children.Count; i++)
        {
            if (i > 0) AddSlot(grid, vertical, new GridLength(SplitterThickness), 0);
            AddSlot(grid, vertical, new GridLength(split.Children[i].Weight, GridUnitType.Star), MinPaneSize);
        }

        for (var i = 0; i < split.Children.Count; i++)
        {
            var slot = i * 2;
            if (i > 0)
                grid.Children.Add(Splitter(grid, split, vertical, slot - 1));

            var child = BuildNode(split.Children[i]);
            if (vertical) Grid.SetColumn(child, slot);
            else Grid.SetRow(child, slot);
            grid.Children.Add(child);
        }

        return grid;
    }

    private static void AddSlot(Grid grid, bool vertical, GridLength length, double minimum)
    {
        if (vertical)
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = length, MinWidth = minimum });
        else
            grid.RowDefinitions.Add(new RowDefinition { Height = length, MinHeight = minimum });
    }

    private GridSplitter Splitter(Grid grid, LayoutSplit<PaneViewModel> split, bool vertical, int slot)
    {
        var splitter = new GridSplitter
        {
            ShowsPreview = false,
            ResizeBehavior = GridResizeBehavior.PreviousAndNext,
            ResizeDirection = vertical ? GridResizeDirection.Columns : GridResizeDirection.Rows,
            HorizontalAlignment = vertical ? HorizontalAlignment.Stretch : HorizontalAlignment.Stretch,
            VerticalAlignment = VerticalAlignment.Stretch,
            Cursor = vertical ? System.Windows.Input.Cursors.SizeWE : System.Windows.Input.Cursors.SizeNS,
        };
        // Same style as the sidebar splitter in XAML, so a code-built sash is themed like the rest.
        splitter.SetResourceReference(FrameworkElement.StyleProperty, "PaneSplitterStyle");

        if (vertical) Grid.SetColumn(splitter, slot);
        else Grid.SetRow(splitter, slot);

        // Without writing the dragged sizes back into the tree, the next split or close would
        // rebuild the grid from stale weights and undo the drag.
        splitter.DragCompleted += (_, _) => WriteBackWeights(grid, split, vertical);
        return splitter;
    }

    private static void WriteBackWeights(Grid grid, LayoutSplit<PaneViewModel> split, bool vertical)
    {
        for (var i = 0; i < split.Children.Count; i++)
        {
            var slot = i * 2;
            var size = vertical
                ? grid.ColumnDefinitions[slot].ActualWidth
                : grid.RowDefinitions[slot].ActualHeight;
            if (size > 0) split.Children[i].Weight = size;
        }
    }

    // --- Scroll preservation across a rebuild ---

    private Dictionary<PaneViewModel, double> CaptureScrollOffsets(HashSet<PaneViewModel> live)
    {
        var offsets = new Dictionary<PaneViewModel, double>();
        foreach (var (pane, view) in _views)
        {
            if (!live.Contains(pane)) continue;
            if (ScrollerFor(view) is { } scroller)
                offsets[pane] = scroller.VerticalOffset;
        }
        return offsets;
    }

    private void RestoreScrollOffsets(Dictionary<PaneViewModel, double> offsets)
    {
        foreach (var (pane, offset) in offsets)
        {
            if (offset <= 0) continue;
            if (ViewFor(pane) is { } view && ScrollerFor(view) is { } scroller)
                scroller.ScrollToVerticalOffset(offset);
        }
    }

    private static ScrollViewer? ScrollerFor(FilePaneView view) =>
        view.ActiveTabView is { } tab ? VisualTreeUtil.FindDescendant<ScrollViewer>(tab) : null;
}
