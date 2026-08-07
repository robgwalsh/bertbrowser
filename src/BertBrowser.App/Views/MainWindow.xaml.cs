using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

public partial class MainWindow : Window
{
    private readonly ShellViewModel _shell;
    private readonly BertBrowser.App.Services.AppSettings _settings;
    private readonly MarqueeSelector _marquee;
    private readonly FileDragDropController _dragDrop;

    public MainWindow(ShellViewModel shell, BertBrowser.App.Services.AppSettings settings)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        DataContext = shell;

        _marquee = MarqueeSelector.Attach(FileListView);
        // Attached after the marquee so the two never fight: the marquee ignores presses that land
        // on a row, and this ignores presses that land on empty space.
        _dragDrop = FileDragDropController.Attach(FileListView, FolderTree, shell, AskAboutConflicts);

        ApplyWindowSettings();

        _shell.FileList.PropertyChanged += FileList_PropertyChanged;
        _shell.PropertyChanged += Shell_PropertyChanged;
        _shell.RevealFileRequested += OnRevealFileRequested;
        UpdateRelPathColumn();
        ApplyViewMode(); // honor a restored thumbnail zoom level

        Loaded += async (_, _) => await _shell.InitializeAsync();
        Closing += (_, _) =>
        {
            SaveWindowSettings();
            // The pending undo is gone once we exit, so commit whatever a Replace set aside rather
            // than leaving hidden staging folders behind.
            _shell.RetireUndoableTransfer();
        };
    }

    /// <summary>Shown when a drop would land on names that are already taken. Returns null when
    /// the user cancels, which abandons the whole drop.</summary>
    private ConflictResolution? AskAboutConflicts(TransferPlan plan)
    {
        var dialog = new TransferConflictDialog(new TransferConflictsViewModel(plan)) { Owner = this };
        return dialog.ShowDialog() == true ? dialog.Resolution : null;
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
        _settings.LastPath = _shell.CurrentPath.Length > 0 && !IsHiddenDirectory(_shell.CurrentPath)
            ? _shell.CurrentPath
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

    private void FileList_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(FileListViewModel.IsFlattened))
            UpdateRelPathColumn();
        else if (e.PropertyName == nameof(FileListViewModel.Items))
            FocusFileList();
        else if (e.PropertyName == nameof(FileListViewModel.IsThumbnailView))
            ApplyViewMode();
    }

    private bool? _thumbnailViewApplied;

    /// <summary>Swaps the file list between the details <see cref="GridView"/> and the
    /// thumbnail-tile layout to match <see cref="FileListViewModel.IsThumbnailView"/>.
    /// One ListView is reused so all its interactions (selection, context menu, double-click,
    /// type-ahead) work in both modes.</summary>
    private void ApplyViewMode()
    {
        var thumbnails = _shell.FileList.IsThumbnailView;
        if (_thumbnailViewApplied == thumbnails) return; // only churn the view on a real change
        _thumbnailViewApplied = thumbnails;

        if (thumbnails)
        {
            FileListView.View = null; // a null View lets ItemsPanel/the template selector take over
            FileListView.ItemsPanel = (ItemsPanelTemplate)FindResource("ThumbPanel");
            // A selector renders media as tiles and folders/non-media as full-width rows.
            FileListView.ItemTemplateSelector = (DataTemplateSelector)FindResource("ThumbOrRowSelector");
            FileListView.ItemContainerStyle = (Style)FindResource("ThumbItemStyle");
            // Disabling the horizontal scrollbar bounds the WrapPanel to the viewport width so
            // tiles roll onto the next row; only a vertical scrollbar ever appears.
            ScrollViewer.SetHorizontalScrollBarVisibility(FileListView, ScrollBarVisibility.Disabled);
        }
        else
        {
            FileListView.View = DetailsView;
            FileListView.ClearValue(ItemsControl.ItemsPanelProperty);           // restore virtualizing stack
            FileListView.ClearValue(ItemsControl.ItemTemplateSelectorProperty); // GridView supplies cells
            FileListView.ItemContainerStyle = (Style)FindResource("FileRowStyle");
            FileListView.ClearValue(ScrollViewer.HorizontalScrollBarVisibilityProperty); // columns can scroll again
        }
    }

    /// <summary>Gives the file list keyboard focus after it reloads so arrow keys and type-ahead
    /// work without a click first — unless the user is typing in the search or path box.</summary>
    private void FocusFileList()
    {
        if (SearchBox.IsKeyboardFocusWithin || PathBox.IsKeyboardFocusWithin) return;
        Dispatcher.InvokeAsync(() => FileListView.Focus(), DispatcherPriority.Input);
    }

    /// <summary>The Folder column only makes sense in the flattened search-results list.</summary>
    private void UpdateRelPathColumn() =>
        RelPathColumn.Width = _shell.FileList.IsFlattened ? 220 : 0;

    // --- Toolbar / dialogs ---

    private void Settings_Click(object sender, RoutedEventArgs e)
    {
        var vm = new SettingsViewModel(_settings);
        if (new SettingsWindow(vm) { Owner = this }.ShowDialog() == true)
        {
            // Sync the toolbar toggle to a "Show hidden items" change made in the dialog; its
            // setter refreshes the list and re-filters bookmarks. (Custom-command menus rebuild
            // on every open, so they need no refresh.)
            _shell.ShowHiddenItems = _settings.ShowHiddenItems;
        }
    }

    /// <summary>Applies the configurable scroll-speed multiplier to mouse-wheel scrolling.
    /// Reproduces WPF's default (WheelScrollLines lines per notch) scaled by the setting, so
    /// 1× matches the system and 2× (the default) is twice as fast.</summary>
    private void Scroll_PreviewMouseWheel(object sender, MouseWheelEventArgs e)
    {
        // Let Shift+wheel do its native horizontal scroll untouched.
        if (e.Delta == 0 || e.Handled || Keyboard.Modifiers.HasFlag(ModifierKeys.Shift)) return;

        var scrollViewer = FindScrollViewer((DependencyObject)sender);
        if (scrollViewer is null) return;

        var wheelLines = SystemParameters.WheelScrollLines;
        if (wheelLines <= 0) wheelLines = 3; // -1 = "one page"; fall back to the common default
        var lines = (int)Math.Round(Math.Abs(e.Delta) / 120.0 * wheelLines * _settings.ScrollSpeedMultiplier);
        lines = Math.Clamp(lines, 1, 240);

        for (var i = 0; i < lines; i++)
        {
            if (e.Delta > 0) scrollViewer.LineUp();
            else scrollViewer.LineDown();
        }
        e.Handled = true;
    }

    private static ScrollViewer? FindScrollViewer(DependencyObject root)
    {
        if (root is ScrollViewer sv) return sv;
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            if (FindScrollViewer(VisualTreeHelper.GetChild(root, i)) is { } found)
                return found;
        }
        return null;
    }

    // --- Breadcrumb ---

    private void BreadcrumbSegment_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string path })
            _ = _shell.NavigateToAsync(path);
        e.Handled = true;
    }

    private void Breadcrumb_EmptyClick(object sender, MouseButtonEventArgs e)
    {
        PathBox.Text = _shell.CurrentPath;
        Breadcrumb.Visibility = Visibility.Collapsed;
        PathBox.Visibility = Visibility.Visible;
        PathBox.Focus();
        PathBox.SelectAll();
    }

    private void PathBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
        {
            var path = PathBox.Text.Trim();
            HidePathBox();
            if (path.Length > 0)
                _ = _shell.NavigateToAsync(path);
        }
        else if (e.Key == Key.Escape)
        {
            HidePathBox();
        }
    }

    private void PathBox_LostFocus(object sender, RoutedEventArgs e) => HidePathBox();

    // --- Search box ---

    private void SearchBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Escape)
        {
            _shell.ClearSearchCommand.Execute(null);
            FileListView.Focus();
            e.Handled = true;
        }
    }

    protected override void OnPreviewKeyDown(KeyEventArgs e)
    {
        if ((e.Key == Key.F || e.Key == Key.E) && Keyboard.Modifiers == ModifierKeys.Control)
        {
            SearchBox.Focus();
            SearchBox.SelectAll();
            e.Handled = true;
        }
        // Undo the last drag-and-drop move. Skipped while a text box has focus so Ctrl+Z still
        // undoes typing there.
        else if (e.Key == Key.Z && Keyboard.Modifiers == ModifierKeys.Control &&
                 !SearchBox.IsKeyboardFocusWithin && !PathBox.IsKeyboardFocusWithin)
        {
            if (_shell.UndoTransferCommand.CanExecute(null))
                _shell.UndoTransferCommand.Execute(null);
            e.Handled = true;
        }
        else if (FileListView.IsKeyboardFocusWithin && Keyboard.Modifiers == ModifierKeys.Control &&
                 e.Key is Key.C or Key.X or Key.V)
        {
            switch (e.Key)
            {
                case Key.C: _shell.CopySelectionCommand.Execute(SelectedFileItems()); break;
                case Key.X: _shell.CutSelectionCommand.Execute(SelectedFileItems()); break;
                case Key.V: _shell.PasteCommand.Execute(null); break;
            }
            e.Handled = true;
        }
        // Alt combinations arrive as Key.System with the real key in SystemKey.
        else if (e.Key == Key.System && e.SystemKey == Key.Enter &&
                 Keyboard.Modifiers == ModifierKeys.Alt &&
                 FileListView.IsKeyboardFocusWithin &&
                 SelectedFileItems() is { Count: > 0 } selected)
        {
            ShowProperties(selected);
            e.Handled = true;
        }
        base.OnPreviewKeyDown(e);
    }

    private void HidePathBox()
    {
        PathBox.Visibility = Visibility.Collapsed;
        Breadcrumb.Visibility = Visibility.Visible;
    }

    // --- File list interactions ---

    private void FileList_HeaderClick(object sender, RoutedEventArgs e)
    {
        if (e.OriginalSource is GridViewColumnHeader { Tag: string tag } &&
            Enum.TryParse<SortColumn>(tag, out var column))
        {
            _shell.FileList.SetSort(column);
        }
    }

    /// <summary>Updates the status-bar selection summary and mirrors a single-item selection into
    /// the folder tree (the item's own folder for directories, its parent for files), expanding and
    /// scrolling as needed. Multi-selection has no one folder to reveal, and a rubber-band drag
    /// churns the selection every frame, so both skip the tree work.</summary>
    private async void FileList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        QueueSelectionSummary();

        if (_marquee.IsDragging || FileListView.SelectedItems.Count != 1) return;
        if (FileListView.SelectedItem is not FileItemViewModel item) return;

        var dir = item.IsDirectory ? item.FullPath : Path.GetDirectoryName(item.FullPath);
        if (string.IsNullOrEmpty(dir)) return;

        // Revealing expands the tree down to this folder; the enumeration + per-child disk probes
        // run off the UI thread, so this awaits rather than blocking. Best-effort UI sugar — a
        // failure to reveal must never crash the async-void handler.
        IReadOnlyList<DirectoryNodeViewModel> chain;
        try
        {
            chain = await _shell.Tree.RevealPathAsync(dir);
        }
        catch
        {
            return;
        }
        if (chain.Count == 0) return;

        // Containers for freshly expanded nodes only exist after a layout pass.
        _ = Dispatcher.InvokeAsync(() => ScrollTreeChainIntoView(chain), DispatcherPriority.Loaded);
    }

    private bool _selectionSummaryPending;

    /// <summary>Coalesces the summary refresh to one per frame: a rubber-band drag adds and removes
    /// items one at a time, and each recount walks the whole selection.</summary>
    private void QueueSelectionSummary()
    {
        if (_selectionSummaryPending) return;
        _selectionSummaryPending = true;
        _ = Dispatcher.InvokeAsync(() =>
        {
            _selectionSummaryPending = false;
            UpdateSelectionSummary();
        }, DispatcherPriority.Background);
    }

    /// <summary>Explorer-style "N items selected (size)" in the status bar. Folder sizes count
    /// only when they've already been computed, so the total never blocks on a scan.</summary>
    private void UpdateSelectionSummary()
    {
        var selected = FileListView.SelectedItems;
        if (selected.Count == 0)
        {
            _shell.SelectionSummary = "";
            return;
        }

        var bytes = 0L;
        foreach (var entry in selected)
        {
            if (entry is FileItemViewModel { SizeBytes: { } size })
                bytes += size;
        }

        var noun = selected.Count == 1 ? "item" : "items";
        _shell.SelectionSummary = bytes > 0
            ? $"{selected.Count:N0} {noun} selected ({BertBrowser.Core.Services.ByteSizeFormatter.Format(bytes)})"
            : $"{selected.Count:N0} {noun} selected";
    }

    /// <summary>Positions the revealed node roughly 40% down the tree's viewport.</summary>
    private void ScrollTreeChainIntoView(IReadOnlyList<DirectoryNodeViewModel> chain)
    {
        // A click that expanded a visible row anchors it under the cursor; re-pin there instead of
        // repositioning, so navigating into a folder by clicking it doesn't make the tree jump.
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

    private static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T match) return match;
            if (FindDescendant<T>(child) is { } nested) return nested;
        }
        return null;
    }

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

    private void Shell_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ShellViewModel.CurrentPath))
            _ = RevealCurrentDirAsync();
    }

    /// <summary>Expands the tree down to the current directory, selects it, scrolls it into view,
    /// and remembers it as the pinned-header target. Best-effort UI sugar — never throws.</summary>
    private async Task RevealCurrentDirAsync()
    {
        var path = _shell.CurrentPath;
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

    /// <summary>WPF doesn't select on right-click, so without this the context menu would act on a
    /// stale selection. Right-clicking inside the selection keeps it (that's how you get a menu for
    /// many items); right-clicking outside narrows it to the row under the cursor, like Explorer.</summary>
    private void FileList_PreviewMouseRightButtonDown(object sender, MouseButtonEventArgs e)
    {
        var d = e.OriginalSource as DependencyObject;
        while (d is not null and not ListViewItem and not ListView)
            d = VisualTreeHelper.GetParent(d);

        if (d is not ListViewItem { DataContext: FileItemViewModel item }) return;
        if (FileListView.SelectedItems.Contains(item)) return;

        FileListView.SelectedItem = item;
    }

    private void FileList_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenItemCommand.Execute(item);
    }

    /// <summary>Enter opens the selected item, like double-click.</summary>
    private void FileList_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && FileListView.SelectedItem is FileItemViewModel item)
        {
            _shell.OpenItemCommand.Execute(item);
            e.Handled = true;
        }
    }

    // --- Type-ahead selection ---

    /// <summary>How long typed characters accumulate into one prefix before the buffer resets.</summary>
    private const long TypeAheadTimeoutMs = 700;
    private string _typeAheadPrefix = "";
    private long _typeAheadTick;

    /// <summary>Explorer-style type-to-select: a single letter jumps to (and cycles through) items
    /// starting with it; letters typed in quick succession match a longer name prefix.</summary>
    private void FileList_TextInput(object sender, TextCompositionEventArgs e)
    {
        if ((Keyboard.Modifiers & (ModifierKeys.Control | ModifierKeys.Alt)) != 0) return;
        var text = e.Text;
        if (text.Length != 1 || char.IsControl(text[0])) return;

        var now = Environment.TickCount64;
        if (now - _typeAheadTick > TypeAheadTimeoutMs)
            _typeAheadPrefix = "";
        _typeAheadTick = now;

        var selected = FileListView.SelectedIndex;

        // Extending the current prefix: keep the selection if it still matches, else search forward
        // from it. Skipped for the first keystroke and when the longer prefix matches nothing.
        if (_typeAheadPrefix.Length > 0 && SelectByPrefix(_typeAheadPrefix + text, selected < 0 ? 0 : selected))
        {
            _typeAheadPrefix += text;
            e.Handled = true;
            return;
        }

        // Fresh letter (or a broken prefix): treat this keystroke as a single-letter jump that
        // advances past the current item, so repeating the same letter cycles through matches.
        if (SelectByPrefix(text, selected + 1))
        {
            _typeAheadPrefix = text;
            e.Handled = true;
        }
    }

    /// <summary>Selects the first item whose name starts with <paramref name="prefix"/>, scanning
    /// forward from <paramref name="start"/> and wrapping around. Returns false if none match.</summary>
    private bool SelectByPrefix(string prefix, int start)
    {
        var items = FileListView.Items;
        var count = items.Count;
        if (count == 0) return false;
        if (start < 0) start = 0;

        for (var i = 0; i < count; i++)
        {
            var idx = (start + i) % count;
            if (items[idx] is FileItemViewModel vm &&
                vm.Name.StartsWith(prefix, StringComparison.CurrentCultureIgnoreCase))
            {
                FileListView.SelectedIndex = idx;
                FileListView.ScrollIntoView(items[idx]);
                (FileListView.ItemContainerGenerator.ContainerFromIndex(idx) as ListViewItem)?.Focus();
                return true;
            }
        }
        return false;
    }

    private void ContextOpen_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenItemCommand.Execute(item);
    }

    private void ContextOpenTerminal_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenInTerminal(item.FullPath, item.IsDirectory);
    }

    private void ContextOpenVSCode_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenInVSCode(item.FullPath, item.IsDirectory);
    }

    // --- Clipboard + custom commands ---

    private List<FileItemViewModel> SelectedFileItems() =>
        FileListView.SelectedItems.Cast<FileItemViewModel>().ToList();

    private void ContextCopy_Click(object sender, RoutedEventArgs e) =>
        _shell.CopySelectionCommand.Execute(SelectedFileItems());

    private void ContextCut_Click(object sender, RoutedEventArgs e) =>
        _shell.CutSelectionCommand.Execute(SelectedFileItems());

    private void ContextPaste_Click(object sender, RoutedEventArgs e) =>
        _shell.PasteCommand.Execute(null);

    /// <summary>Enables clipboard items for the current state and rebuilds the
    /// user-defined command entries for the selection.</summary>
    private void FileList_ContextMenuOpening(object sender, ContextMenuEventArgs e)
    {
        if (FileListView.ContextMenu is not { } menu) return;

        var selection = SelectedFileItems();
        CopyMenuItem.IsEnabled = CutMenuItem.IsEnabled = selection.Count > 0;
        PasteMenuItem.IsEnabled = BertBrowser.App.Services.FileClipboard.HasFiles();

        BookmarkMenuItem.IsEnabled = selection.Count > 0;
        // "Remove bookmark" only when every selected item is already bookmarked.
        var allBookmarked = selection.Count > 0 && selection.All(i => _shell.Bookmarks.IsBookmarked(i.FullPath));
        BookmarkMenuItem.Header = allBookmarked ? "Remove bookmark" : "Bookmark";

        RebuildCustomCommandItems(menu, CustomCommandsSeparator,
            selection.Select(i => (i.FullPath, i.IsDirectory)).ToList());
    }

    /// <summary>Replaces the custom-command section of a context menu (everything tagged
    /// with a CustomCommandDefinition) with the entries applicable to the given targets.</summary>
    private void RebuildCustomCommandItems(
        ContextMenu menu, Separator anchor, IReadOnlyList<(string FullPath, bool IsDirectory)> targets)
    {
        for (var i = menu.Items.Count - 1; i >= 0; i--)
        {
            if (menu.Items[i] is MenuItem { Tag: BertBrowser.App.Services.CustomCommandDefinition })
                menu.Items.RemoveAt(i);
        }

        var applicable = _settings.CustomCommands
            .Where(c => targets.Any(t => t.IsDirectory ? c.AppliesToDirectories : c.AppliesToFiles))
            .ToList();
        anchor.Visibility = applicable.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        var insertAt = menu.Items.IndexOf(anchor) + 1;
        foreach (var definition in applicable)
        {
            // "__" so underscores in names render instead of becoming access keys.
            var item = new MenuItem
            {
                Header = definition.Name.Replace("_", "__"),
                Tag = definition,
                Icon = new TextBlock
                {
                    // E8A7 = OpenInNewWindow: reads as "launch externally".
                    Text = "",
                    FontFamily = new FontFamily("Segoe Fluent Icons, Segoe MDL2 Assets"),
                    FontSize = 16,
                    Foreground = new SolidColorBrush(Color.FromRgb(0x44, 0x44, 0x44)),
                },
            };
            item.Click += (_, _) => _shell.RunCustomCommand(definition, targets);
            menu.Items.Insert(insertAt++, item);
        }
    }

    private void ContextComputeSize_Click(object sender, RoutedEventArgs e)
    {
        var items = FileListView.SelectedItems.Cast<FileItemViewModel>().ToList();
        _shell.ComputeSizeCommand.Execute(items);
    }

    // --- Bookmarks ---

    private void ContextBookmark_Click(object sender, RoutedEventArgs e)
    {
        var entries = SelectedFileItems().Select(i => (i.FullPath, i.IsDirectory)).ToList();
        _ = _shell.ToggleBookmarksAsync(entries);
    }

    private void BookmarkRow_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is BookmarkItemViewModel item)
            _ = _shell.OpenBookmarkAsync(item);
    }

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

    /// <summary>Selects and scrolls to a freshly-loaded file (e.g. after opening a bookmarked file).</summary>
    private void OnRevealFileRequested(string fullPath)
    {
        Dispatcher.InvokeAsync(() =>
        {
            foreach (var obj in FileListView.Items)
            {
                if (obj is FileItemViewModel vm &&
                    string.Equals(vm.FullPath, fullPath, StringComparison.OrdinalIgnoreCase))
                {
                    FileListView.SelectedItem = vm;
                    FileListView.ScrollIntoView(vm);
                    (FileListView.ItemContainerGenerator.ContainerFromItem(vm) as ListViewItem)?.Focus();
                    break;
                }
            }
        }, DispatcherPriority.Loaded);
    }

    // --- Properties dialog ---

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
                RebuildCustomCommandItems(menu, TreeCustomCommandsSeparator, [(node.FullPath, true)]);
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

    // A folder row the user clicked to expand/collapse, pinned to the viewport position it had at
    // the moment of the click so its subtree grows/shrinks *below* it and the row itself stays put
    // under the cursor. _treeAnchorViewportY is that row top's offset from the tree viewport.
    private DirectoryNodeViewModel? _treeAnchorNode;
    private TreeViewItem? _treeAnchorContainer;
    private double _treeAnchorViewportY;

    private void FolderTreeItem_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: DirectoryNodeViewModel node })
        {
            _treeItemMouseDownNode = node;
            _treeItemExpandedAtMouseDown = node.IsExpanded;
        }
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
            // Pin the row where it sits now, before the toggle reflows the tree. Without this the
            // navigation reveal-scroll (or an offset clamp when collapsing near the bottom) slides
            // the clicked row off the cursor, which reads as an awkward jump.
            AnchorTreeRow(node, FindAncestorTreeViewItem(sender as DependencyObject));

            node.IsExpanded = !_treeItemExpandedAtMouseDown;

            // When the click also navigates, RevealCurrentDirAsync's scroll honors the anchor; when
            // it doesn't (re-clicking the current folder to collapse it) re-pin here once the toggle
            // has settled. Exactly one of the two paths runs, so they never fight over the offset.
            if (node.FullPath.Equals(_shell.CurrentPath, StringComparison.OrdinalIgnoreCase))
                _ = Dispatcher.InvokeAsync(RestoreTreeAnchor, DispatcherPriority.Loaded);
        }
        else
        {
            ClearTreeAnchor();
        }
        _treeItemMouseDownNode = null;
    }

    /// <summary>Records the anchored row and its current viewport offset. Cleared (no anchor) when
    /// the row has no realized container or scroll viewer to measure against.</summary>
    private void AnchorTreeRow(DirectoryNodeViewModel node, TreeViewItem? container)
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

    /// <summary>Scrolls the tree so the anchored (just expanded/collapsed) row returns to the exact
    /// viewport offset it had when clicked, keeping it under the cursor. One-shot: clears the anchor
    /// so a later, unrelated reveal isn't pinned to a stale position.</summary>
    private void RestoreTreeAnchor()
    {
        var container = _treeAnchorContainer;
        var targetY = _treeAnchorViewportY;
        ClearTreeAnchor();
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

    private static TreeViewItem? FindAncestorTreeViewItem(DependencyObject? d)
    {
        while (d is not null and not TreeViewItem)
            d = VisualTreeHelper.GetParent(d);
        return d as TreeViewItem;
    }

    private void TreeProperties_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            ShowProperties(node.FullPath, isDirectory: true);
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

    private void ContextProperties_Click(object sender, RoutedEventArgs e)
    {
        if (SelectedFileItems() is { Count: > 0 } selection)
            ShowProperties(selection);
    }

    /// <summary>Opens the properties dialog for a whole selection; with more than one item it
    /// shows aggregates and edits the shared attributes in bulk.</summary>
    private void ShowProperties(IReadOnlyList<FileItemViewModel> items) =>
        ShowProperties(items.Select(i => new PropertiesTarget(i.FullPath, i.IsDirectory)).ToList());

    private void ShowProperties(string fullPath, bool isDirectory) =>
        ShowProperties([new PropertiesTarget(fullPath, isDirectory)]);

    private void ShowProperties(IReadOnlyList<PropertiesTarget> targets)
    {
        if (targets.Count == 0) return;

        var vm = new PropertiesViewModel(targets,
            App.Services.GetRequiredService<IDirectorySizeService>(),
            App.Services.GetRequiredService<DirSizeRepository>());
        new PropertiesDialog(vm) { Owner = this }.ShowDialog();
        if (vm.AttributesChanged)
            _shell.RefreshCommand.Execute(null); // hidden-bit toggles can add/remove rows
    }
}
