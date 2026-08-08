using BertBrowser.App.Theming;
using Microsoft.Extensions.DependencyInjection;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;

namespace BertBrowser.App.Views;

public partial class MainWindow : ThemedWindow
{
    private readonly ShellViewModel _shell;
    private readonly BertBrowser.App.Services.AppSettings _settings;
    private readonly PaneLayoutHost _layoutHost;

    public MainWindow(ShellViewModel shell, BertBrowser.App.Services.AppSettings settings)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        DataContext = shell;

        _layoutHost = new PaneLayoutHost(shell, settings);
        PaneHostSite.Child = _layoutHost;

        // Attached once, not per pane: the tree is shared, so N file-list controllers each hooking
        // its Drop would carry the same transfer out once per open pane.
        TreeDropTarget.Attach(FolderTree, shell);

        ApplyWindowSettings();

        _shell.ActiveLocationChanged += OnActiveLocationChanged;
        _shell.TreeRevealRequested += OnTreeRevealRequested;
        _shell.PaneFocusRequested += OnPaneFocusRequested;

        Loaded += async (_, _) => await _shell.InitializeAsync();
        Closing += (_, _) =>
        {
            SaveWindowSettings();
            // The pending undo is gone once we exit, so commit whatever a Replace set aside rather
            // than leaving hidden staging folders behind.
            _shell.RetireUndoableTransfer();
        };
    }

    private void ApplyWindowSettings()
    {
        if (_settings is { WindowWidth: > 200, WindowHeight: > 150 })
        {
            Width = _settings.WindowWidth!.Value;
            Height = _settings.WindowHeight!.Value;
        }
        if (_settings is { WindowLeft: { } left, WindowTop: { } top } &&
            left > SystemParameters.VirtualScreenLeft - 100 &&
            top > SystemParameters.VirtualScreenTop - 100 &&
            left < SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth - 100 &&
            top < SystemParameters.VirtualScreenTop + SystemParameters.VirtualScreenHeight - 100)
        {
            WindowStartupLocation = WindowStartupLocation.Manual;
            Left = left;
            Top = top;
        }
        if (_settings.WindowMaximized)
            WindowState = WindowState.Maximized;
    }

    private void SaveWindowSettings()
    {
        var bounds = WindowState == WindowState.Normal
            ? new Rect(Left, Top, Width, Height)
            : RestoreBounds;
        _settings.WindowLeft = bounds.Left;
        _settings.WindowTop = bounds.Top;
        _settings.WindowWidth = bounds.Width;
        _settings.WindowHeight = bounds.Height;
        _settings.WindowMaximized = WindowState == WindowState.Maximized;
        // Restore the last directory next launch — but never a hidden folder.
        _settings.LastPath = _shell.ActiveTab.CurrentPath.Length > 0 && !IsHiddenDirectory(_shell.ActiveTab.CurrentPath)
            ? _shell.ActiveTab.CurrentPath
            : null;
        _settings.Save(); // per-directory thumbnail scales are already updated live in the map
    }

    private static bool IsHiddenDirectory(string path)
    {
        try
        {
            return new DirectoryInfo(path).Attributes.HasFlag(FileAttributes.Hidden);
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Moves real keyboard focus into a pane after a split or an F6 — the one thing the
    /// view models can't do for themselves.</summary>
    private void OnPaneFocusRequested(PaneViewModel pane) =>
        _layoutHost.ViewFor(pane)?.FocusActiveTabList();

    // --- Toolbar / dialogs ---

    private void Settings_Click(object sender, RoutedEventArgs e)
    {
        var vm = new SettingsViewModel(_settings, App.Services.GetRequiredService<IThemeService>());
        if (new SettingsWindow(vm) { Owner = this }.ShowDialog() == true)
        {
            // Sync the toolbar toggle to a "Show hidden items" change made in the dialog; its
            // setter refreshes the list and re-filters bookmarks. (Custom-command menus rebuild
            // on every open, so they need no refresh.)
            _shell.ShowHiddenItems = _settings.ShowHiddenItems;
        }
    }

    private void Scroll_PreviewMouseWheel(object sender, MouseWheelEventArgs e)
    {
        // Scrolling the tree by hand outranks any row pinned by an earlier click — but only when
        // the wheel notch is one this handler will actually act on.
        if (ReferenceEquals(sender, FolderTree) && e.Delta != 0 && !e.Handled &&
            !Keyboard.Modifiers.HasFlag(ModifierKeys.Shift))
        {
            ClearTreeAnchor();
        }

        ScrollSpeed.HandlePreviewMouseWheel(sender, e, _settings);
    }

    /// <summary>Only the genuinely window-wide shortcut lives here. Everything that acts on a
    /// directory — navigation, clipboard, properties, focusing the search box — belongs to the pane
    /// that has focus, and is handled in <see cref="DirectoryTabView"/> or bound through
    /// <c>ActivePane</c> in XAML.</summary>
    protected override void OnPreviewKeyDown(KeyEventArgs e)
    {
        // Focusing search is window-wide so it works from the sidebar too; which box it lands in
        // is decided by which pane is active.
        if ((e.Key == Key.F || e.Key == Key.E) && Keyboard.Modifiers == ModifierKeys.Control)
        {
            _layoutHost.ActivePaneView?.FocusSearchBox();
            e.Handled = true;
        }
        // Undo the last drag-and-drop move. Skipped while any text box has focus so Ctrl+Z still
        // undoes typing there — and there is one search box and one path box per open tab now, so
        // the test has to be about the focused element rather than about named controls.
        else if (e.Key == Key.Z && Keyboard.Modifiers == ModifierKeys.Control &&
            Keyboard.FocusedElement is not TextBoxBase)
        {
            if (_shell.UndoTransferCommand.CanExecute(null))
                _shell.UndoTransferCommand.Execute(null);
            e.Handled = true;
        }
        base.OnPreviewKeyDown(e);
    }

    /// <summary>Expands the tree down to a folder and scrolls it into view. Revealing runs the
    /// enumeration and per-child disk probes off the UI thread, so this awaits rather than
    /// blocking. Best-effort UI sugar — a failure to reveal must never crash the async-void
    /// handler.</summary>
    private async void OnTreeRevealRequested(string directory)
    {
        IReadOnlyList<DirectoryNodeViewModel> chain;
        try
        {
            chain = await _shell.Tree.RevealPathAsync(directory);
        }
        catch
        {
            return;
        }
        if (chain.Count == 0) return;

        // Containers for freshly expanded nodes only exist after a layout pass.
        _ = Dispatcher.InvokeAsync(() => ScrollTreeChainIntoView(chain), DispatcherPriority.Loaded);
    }
    /// <summary>Positions the revealed node roughly 40% down the tree's viewport.</summary>
    private void ScrollTreeChainIntoView(IReadOnlyList<DirectoryNodeViewModel> chain)
    {
        // A click in the tree anchors the row it landed on; re-pin there instead of repositioning,
        // so navigating into a folder by clicking it doesn't make the tree jump. (An anchor only
        // survives while the shell still sits on the row that was clicked — see Shell_PropertyChanged
        // — so an unrelated reveal of that same folder later still gets the 40% positioning.)
        if (chain.Count > 0 && ReferenceEquals(_treeAnchorNode, chain[^1]))
        {
            RestoreTreeAnchor();
            return;
        }

        ItemsControl parent = FolderTree;
        TreeViewItem? container = null;
        foreach (var node in chain)
        {
            parent.UpdateLayout();
            container = parent.ItemContainerGenerator.ContainerFromItem(node) as TreeViewItem;
            if (container is null) return;
            parent = container;
        }
        if (container is null) return;

        var scroller = FindDescendant<ScrollViewer>(FolderTree);
        if (scroller is null)
        {
            container.BringIntoView();
            return;
        }

        try
        {
            var rowTop = container.TransformToAncestor(scroller).Transform(default).Y;
            var target = scroller.VerticalOffset + rowTop - scroller.ViewportHeight * 0.4;
            scroller.ScrollToVerticalOffset(Math.Max(0, target));
        }
        catch (InvalidOperationException)
        {
            container.BringIntoView(); // not connected to the visual tree yet
        }
    }

    private static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject =>
        VisualTreeUtil.FindDescendant<T>(root);

    // --- Pinned current-directory header ---

    // The tree row for the currently open directory, and the root-to-node chain used to locate
    // its container. Kept in sync with the shell's CurrentPath (not the file-list selection) so
    // the pinned header always tracks the folder being browsed.
    private IReadOnlyList<DirectoryNodeViewModel> _currentDirChain = Array.Empty<DirectoryNodeViewModel>();
    private DirectoryNodeViewModel? _currentDirNode;

    /// <summary>Wires the tree's scroll viewer once its template is applied, so the pinned
    /// header can react to every scroll, expand/collapse, and resize.</summary>
    private void FolderTree_Loaded(object sender, RoutedEventArgs e)
    {
        if (FindDescendant<ScrollViewer>(FolderTree) is { } scroller)
        {
            scroller.ScrollChanged -= FolderTreeScrollChanged; // idempotent across re-raises
            scroller.ScrollChanged += FolderTreeScrollChanged;
        }
    }

    private void FolderTreeScrollChanged(object sender, ScrollChangedEventArgs e) => UpdatePinnedRow();

    // A reveal enumerates directories off-thread and reflows the tree, so it must not run once per
    // keystroke of a held-down Ctrl+Tab. The last path wins, one short beat after the churn stops.
    private DispatcherTimer? _revealTimer;
    private string _pendingRevealPath = "";
    private string _revealedPath = "";

    /// <summary>The active directory changed — because the user navigated, or because a different
    /// tab or pane came to the front. Only the active one ever reaches here.</summary>
    private void OnActiveLocationChanged(string path)
    {
        // Navigating anywhere but the clicked row retires its anchor: from here on the reveal is
        // free to position the tree, and a much later return to that folder mustn't snap back to
        // a viewport offset the row held during some earlier click.
        if (_treeAnchorNode is { } anchored &&
            !anchored.FullPath.Equals(path, StringComparison.OrdinalIgnoreCase))
        {
            ClearTreeAnchor();
        }

        if (path.Length == 0 || path.Equals(_revealedPath, StringComparison.OrdinalIgnoreCase)) return;

        _pendingRevealPath = path;
        _revealTimer ??= new DispatcherTimer(
            TimeSpan.FromMilliseconds(120), DispatcherPriority.Background, OnRevealTick, Dispatcher);
        _revealTimer.Stop();
        _revealTimer.Start();
    }

    private void OnRevealTick(object? sender, EventArgs e)
    {
        _revealTimer?.Stop();
        var path = _pendingRevealPath;
        if (path.Length == 0) return;
        _revealedPath = path;
        _ = RevealCurrentDirAsync(path);
    }

    /// <summary>Expands the tree down to the current directory, selects it, scrolls it into view,
    /// and remembers it as the pinned-header target. Best-effort UI sugar — never throws.</summary>
    private async Task RevealCurrentDirAsync(string path)
    {
        if (path.Length == 0) return;

        IReadOnlyList<DirectoryNodeViewModel> chain;
        try
        {
            chain = await _shell.Tree.RevealPathAsync(path);
        }
        catch
        {
            return;
        }

        _currentDirChain = chain;
        _currentDirNode = chain.Count > 0 ? chain[^1] : null;

        // Expand the current directory itself (RevealPathAsync only expands its ancestors) so its
        // subfolders show in the tree and there's something for the pinned header to collapse.
        if (_currentDirNode is not null)
        {
            _currentDirNode.IsExpanded = true;
            await _currentDirNode.EnsurePopulatedAsync(); // children load off-thread; wait before measuring
        }

        // Containers for freshly expanded nodes only exist after a layout pass.
        _ = Dispatcher.InvokeAsync(() =>
        {
            if (chain.Count > 0) ScrollTreeChainIntoView(chain);
            UpdatePinnedRow();
        }, DispatcherPriority.Loaded);
    }

    /// <summary>Shows the pinned header when the current directory's row has scrolled above the
    /// tree's viewport (and only while a vertical scrollbar is present), so the user can always
    /// collapse it. Hides it otherwise.</summary>
    private void UpdatePinnedRow()
    {
        if (_currentDirNode is null || _currentDirChain.Count == 0)
        {
            HidePinnedRow();
            return;
        }

        var scroller = FindDescendant<ScrollViewer>(FolderTree);
        if (scroller is null || scroller.ComputedVerticalScrollBarVisibility != Visibility.Visible)
        {
            HidePinnedRow();
            return;
        }

        var container = ContainerForChain(_currentDirChain);
        if (container is null)
        {
            HidePinnedRow();
            return;
        }

        double rowTop;
        try
        {
            rowTop = container.TransformToAncestor(scroller).Transform(default).Y;
        }
        catch (InvalidOperationException)
        {
            HidePinnedRow(); // not connected to the visual tree yet
            return;
        }

        // container.ActualHeight spans the row plus its expanded subtree; pin while the header is
        // above the top but some of the subtree is still on screen below the pinned bar.
        var subtreeHeight = container.ActualHeight;
        if (rowTop < 0 && rowTop + subtreeHeight > 0)
        {
            PinnedRow.DataContext = _currentDirNode;
            PinnedRow.Visibility = Visibility.Visible;
        }
        else
        {
            HidePinnedRow();
        }
    }

    private void HidePinnedRow()
    {
        PinnedRow.Visibility = Visibility.Collapsed;
        PinnedRow.DataContext = null;
    }

    /// <summary>Walks the root-to-node chain to the current directory's realized container
    /// (virtualization is off, so every expanded node has one after layout).</summary>
    private TreeViewItem? ContainerForChain(IReadOnlyList<DirectoryNodeViewModel> chain)
    {
        ItemsControl parent = FolderTree;
        TreeViewItem? container = null;
        foreach (var node in chain)
        {
            container = parent.ItemContainerGenerator.ContainerFromItem(node) as TreeViewItem;
            if (container is null) return null;
            parent = container;
        }
        return container;
    }

    /// <summary>Clicking the pinned header collapses the current directory and scrolls its
    /// (now collapsed) row back to the top so it's visible again.</summary>
    private void PinnedRow_Click(object sender, MouseButtonEventArgs e)
    {
        if (_currentDirNode is null) return;
        _currentDirNode.IsExpanded = false;
        e.Handled = true;
        ClearTreeAnchor(); // this click's whole point is to move the row back to the top

        var chain = _currentDirChain;
        _ = Dispatcher.InvokeAsync(() =>
        {
            ScrollTreeChainToTop(chain);
            UpdatePinnedRow();
        }, DispatcherPriority.Loaded);
    }

    /// <summary>Scrolls the tree so the chain's node sits flush at the top of the viewport.</summary>
    private void ScrollTreeChainToTop(IReadOnlyList<DirectoryNodeViewModel> chain)
    {
        var scroller = FindDescendant<ScrollViewer>(FolderTree);
        var container = ContainerForChain(chain);
        if (scroller is null || container is null) return;

        try
        {
            var rowTop = container.TransformToAncestor(scroller).Transform(default).Y;
            scroller.ScrollToVerticalOffset(Math.Max(0, scroller.VerticalOffset + rowTop));
        }
        catch (InvalidOperationException)
        {
        }
    }

    private void BookmarkRow_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not BookmarkItemViewModel item) return;

        if (e.ChangedButton == MouseButton.Middle)
        {
            OpenBookmarkInNewTab(item);
            e.Handled = true;
            return;
        }
        if (e.ChangedButton == MouseButton.Left)
            _ = _shell.OpenBookmarkAsync(item);
    }

    private void BookmarkOpenInNewTab_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is BookmarkItemViewModel item)
            OpenBookmarkInNewTab(item);
    }

    private void BookmarkOpenInPaneRight_Click(object sender, RoutedEventArgs e) =>
        OpenBookmarkInNewPane(sender, SplitOrientation.Vertical);

    private void BookmarkOpenInPaneBelow_Click(object sender, RoutedEventArgs e) =>
        OpenBookmarkInNewPane(sender, SplitOrientation.Horizontal);

    private void OpenBookmarkInNewPane(object sender, SplitOrientation orientation)
    {
        if ((sender as FrameworkElement)?.DataContext is not BookmarkItemViewModel item) return;
        _shell.OpenInNewPane(FolderOf(item), orientation);
    }

    /// <summary>A bookmarked file opens its containing folder and then selects the file in the new
    /// tab; a bookmarked folder just opens.</summary>
    private void OpenBookmarkInNewTab(BookmarkItemViewModel item)
    {
        if (FolderOf(item).Length == 0) return;

        // A file bookmark starts the tab empty and lets the reveal do the navigating, so the tab
        // isn't racing two loads of the same folder.
        var tab = _shell.ActivePane.AddTab(item.IsDirectory ? item.FullPath : "", activate: false);
        if (!item.IsDirectory) _ = tab.RevealFileAsync(item.FullPath);
    }

    private static string FolderOf(BookmarkItemViewModel item) =>
        item.IsDirectory ? item.FullPath : Path.GetDirectoryName(item.FullPath) ?? "";

    private void BookmarkOpen_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is BookmarkItemViewModel item)
            _ = _shell.OpenBookmarkAsync(item);
    }

    private void BookmarkRemove_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is BookmarkItemViewModel item)
            _ = _shell.RemoveBookmarkAsync(item);
    }


    private DirectoryNodeViewModel? _treeContextNode;

    /// <summary>Right-click doesn't select in a TreeView, and selecting programmatically
    /// would navigate the shell — so capture the node under the cursor instead.</summary>
    private void FolderTree_ContextMenuOpening(object sender, ContextMenuEventArgs e)
    {
        _treeContextNode = null;
        var d = e.OriginalSource as DependencyObject;
        while (d is not null and not TreeViewItem)
            d = VisualTreeHelper.GetParent(d);
        if (d is TreeViewItem { DataContext: DirectoryNodeViewModel { FullPath.Length: > 0 } node })
        {
            _treeContextNode = node;
            TreeBookmarkMenuItem.Header =
                _shell.Bookmarks.IsBookmarked(node.FullPath) ? "Remove bookmark" : "Bookmark";
            if (FolderTree.ContextMenu is { } menu)
            {
                CustomCommandMenu.Rebuild(menu, TreeCustomCommandsSeparator, [(node.FullPath, true)],
                    _settings, _shell.RunCustomCommand);
            }
        }
        else
        {
            e.Handled = true; // portable device, empty area, or unexpanded placeholder: no menu
        }
    }

    private void TreeBookmark_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _ = _shell.ToggleBookmarksAsync([(node.FullPath, true)]);
    }

    private void TreeOpenInNewTab_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenInNewTab(node.FullPath);
    }

    private void TreeOpenInPaneRight_Click(object sender, RoutedEventArgs e) =>
        OpenTreeNodeInNewPane(SplitOrientation.Vertical);

    private void TreeOpenInPaneBelow_Click(object sender, RoutedEventArgs e) =>
        OpenTreeNodeInNewPane(SplitOrientation.Horizontal);

    private void OpenTreeNodeInNewPane(SplitOrientation orientation)
    {
        if (_treeContextNode is { } node)
            _shell.OpenInNewPane(node.FullPath, orientation);
    }

    /// <summary>Double-clicking a portable device opens it in Explorer (its MTP contents
    /// aren't a filesystem path the in-app list can read).</summary>
    private void FolderTree_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (FolderTree.SelectedItem is PortableDeviceNodeViewModel device)
            BertBrowser.App.Interop.PortableDevices.OpenInExplorer(device.Device);
    }

    // The clicked row and its expansion state at mouse-down, captured before selecting the row
    // navigates to it. Selecting sets the shell's CurrentPath synchronously, and the resulting
    // RevealCurrentDirAsync auto-expands the current directory — so by mouse-up node.IsExpanded
    // may already be true. FolderTreeItem_Click toggles from this pre-click value instead of the
    // live one so the reveal-expand and the click-toggle don't fight (otherwise the first click
    // would open then immediately collapse the folder).
    private DirectoryNodeViewModel? _treeItemMouseDownNode;
    private bool _treeItemExpandedAtMouseDown;

    // The row the user clicked, pinned to the viewport position it had at the moment of the click:
    // whatever the click sets off (selection, navigation reveal, expand/collapse reflow) the row
    // itself must not move under the cursor. _treeAnchorViewportY is that row top's offset from the
    // tree viewport. The anchor stays live until the user scrolls the tree or navigates elsewhere,
    // so every layout pass the click triggers — including ones that land long after mouse-up —
    // re-pins to the same offset.
    private ISidebarNode? _treeAnchorNode;
    private TreeViewItem? _treeAnchorContainer;
    private double _treeAnchorViewportY;

    private void FolderTreeItem_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        ClearTreeAnchor();
        _treeItemMouseDownNode = null;
        if (sender is not FrameworkElement { DataContext: ISidebarNode node }) return;

        // Middle-click opens a background tab instead of navigating. Handling it here also stops
        // TreeViewItem selecting the row, which is what would otherwise navigate the active tab
        // as well.
        if (e.ChangedButton == MouseButton.Middle)
        {
            if (node is DirectoryNodeViewModel { FullPath.Length: > 0 } target)
                _shell.OpenInNewTab(target.FullPath);
            e.Handled = true;
            return;
        }

        if (node is DirectoryNodeViewModel dir)
        {
            _treeItemMouseDownNode = dir;
            _treeItemExpandedAtMouseDown = dir.IsExpanded;
        }

        // Mouse-down is the last moment the tree is still in its pre-click layout: this preview
        // event tunnels ahead of the bubbling one where TreeViewItem selects the row (which focuses
        // it, scrolls it into view, and kicks off the navigation reveal). Measure here, then undo
        // whatever that scrolled once the input event has been fully processed.
        AnchorTreeRow(node, FindAncestorTreeViewItem(sender as DependencyObject));
        ScheduleTreeAnchorRestore();
    }

    /// <summary>A single click on a folder row toggles its expansion (on top of selecting/
    /// navigating), so the tree opens and closes without having to hit the small chevron. Applies
    /// to every folder with children, drives included.</summary>
    private void FolderTreeItem_Click(object sender, MouseButtonEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: DirectoryNodeViewModel node }
            && node.Children.Count > 0
            && ReferenceEquals(node, _treeItemMouseDownNode))
        {
            // The row is already anchored (mouse-down); the toggle reflows the tree below it, and
            // again when lazily-loaded children arrive. Re-pin after each so an offset clamp when
            // collapsing near the bottom — or the reveal-scroll of a click that also navigates —
            // can't slide the row off the cursor. Every restore targets the same offset, so the
            // passes never fight.
            node.IsExpanded = !_treeItemExpandedAtMouseDown;

            ScheduleTreeAnchorRestore();
            if (node.IsExpanded)
                _ = RestoreTreeAnchorAfterPopulateAsync(node);
        }
        _treeItemMouseDownNode = null;
    }

    private void ScheduleTreeAnchorRestore() =>
        _ = Dispatcher.InvokeAsync(RestoreTreeAnchor, DispatcherPriority.Loaded);

    /// <summary>Re-pins once a freshly expanded node's children have loaded and laid out — that
    /// enumeration is off-thread, so its reflow can land well after the click. No-ops if the anchor
    /// has since moved on. Best-effort UI sugar: a failed populate must not crash the handler.</summary>
    private async Task RestoreTreeAnchorAfterPopulateAsync(DirectoryNodeViewModel node)
    {
        try
        {
            await node.EnsurePopulatedAsync();
        }
        catch
        {
            return;
        }
        if (!ReferenceEquals(node, _treeAnchorNode)) return;
        ScheduleTreeAnchorRestore();
    }

    /// <summary>Records the anchored row and its current viewport offset. Cleared (no anchor) when
    /// the row has no realized container or scroll viewer to measure against.</summary>
    private void AnchorTreeRow(ISidebarNode node, TreeViewItem? container)
    {
        ClearTreeAnchor();
        var scroller = FindDescendant<ScrollViewer>(FolderTree);
        if (scroller is null || container is null) return;
        try
        {
            _treeAnchorViewportY = container.TransformToAncestor(scroller).Transform(default).Y;
            _treeAnchorNode = node;
            _treeAnchorContainer = container;
        }
        catch (InvalidOperationException)
        {
        }
    }

    private void ClearTreeAnchor()
    {
        _treeAnchorNode = null;
        _treeAnchorContainer = null;
    }

    /// <summary>Scrolls the tree so the anchored (just clicked) row returns to the exact viewport
    /// offset it had at mouse-down, keeping it under the cursor. Deliberately not one-shot — a click
    /// reflows the tree several times as its navigation, expansion and off-thread child load land —
    /// so the anchor lives until <see cref="ClearTreeAnchor"/> retires it.</summary>
    private void RestoreTreeAnchor()
    {
        var container = _treeAnchorContainer;
        var targetY = _treeAnchorViewportY;
        if (container is null) return;

        var scroller = FindDescendant<ScrollViewer>(FolderTree);
        if (scroller is null) return;
        try
        {
            var rowTop = container.TransformToAncestor(scroller).Transform(default).Y;
            scroller.ScrollToVerticalOffset(Math.Max(0, scroller.VerticalOffset + rowTop - targetY));
        }
        catch (InvalidOperationException)
        {
        }
    }

    private static TreeViewItem? FindAncestorTreeViewItem(DependencyObject? d) =>
        VisualTreeUtil.FindAncestor<TreeViewItem>(d);

    private void TreeProperties_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node && PropertiesPrompt.Show(node.FullPath, isDirectory: true))
            _shell.ActiveTab.RefreshCommand.Execute(null); // hidden-bit toggles can add/remove rows
    }

    private void TreeOpenTerminal_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenInTerminal(node.FullPath, isDirectory: true);
    }

    private void TreeOpenVSCode_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenInVSCode(node.FullPath, isDirectory: true);
    }

}
