using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.App.Views;

/// <summary>
/// The view for one open directory. Everything here is scoped to <see cref="Tab"/> — several of
/// these are alive at once (one per tab, across every pane), so a handler that reached for "the"
/// file list or "the" current directory would act on the wrong one.
/// </summary>
public partial class DirectoryTabView : UserControl
{
    private readonly ShellViewModel _shell;
    private readonly AppSettings _settings;
    private readonly MarqueeSelector _marquee;

    public DirectoryTabViewModel Tab { get; }

    public DirectoryTabView(ShellViewModel shell, AppSettings settings, DirectoryTabViewModel tab)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        Tab = tab;
        DataContext = tab;

        _marquee = MarqueeSelector.Attach(FileListView);
        // The last selection change of a rubber-band sweep lands while the band is still down, and
        // the preview skips those; without this the pane would keep showing whatever was selected
        // before the drag began.
        _marquee.DragEnded += () => { if (Tab.IsPreviewVisible) Tab.Preview.Show(Tab.SelectedItems); };
        // Attached after the marquee so the two never fight: the marquee ignores presses that land
        // on a row, and this ignores presses that land on empty space.
        FileDragDropController.Attach(FileListView, tab, shell);

        Tab.FileList.PropertyChanged += FileList_PropertyChanged;
        Tab.PropertyChanged += Tab_PropertyChanged;
        Tab.RevealFileRequested += OnRevealFileRequested;
        UpdateRelPathColumn();
        ApplyViewMode();     // honor a restored thumbnail zoom level
        UpdatePreviewPane(); // and a restored preview pane
    }

    /// <summary>Gives the subscriptions back. A tab is closable, unlike the window, so its view
    /// has to let go of the view model when it goes.</summary>
    public void Detach()
    {
        Tab.FileList.PropertyChanged -= FileList_PropertyChanged;
        Tab.PropertyChanged -= Tab_PropertyChanged;
        Tab.RevealFileRequested -= OnRevealFileRequested;
        PreviewPane.Detach();
    }

    /// <summary>Keeps the deepest crumb visible: panes are narrower than a window, so a long path
    /// scrolls out of the address bar far more often than it used to.</summary>
    private void Tab_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(DirectoryTabViewModel.PendingSelection))
        {
            // The row may already be on screen — selecting something in the folder already open is
            // the common case, and no reload is coming to trigger it later.
            TryApplyPendingSelection(clearIfMissing: false);
            return;
        }

        if (e.PropertyName == nameof(DirectoryTabViewModel.IsPreviewVisible))
        {
            UpdatePreviewPane();
            return;
        }

        if (e.PropertyName != nameof(DirectoryTabViewModel.CurrentPath)) return;
        _ = Dispatcher.InvokeAsync(BreadcrumbScroller.ScrollToRightEnd, DispatcherPriority.Loaded);
    }

    // --- Preview pane ---

    /// <summary>Shows or hides the preview column. Assigned rather than bound because
    /// <see cref="ColumnDefinition.Width"/> is not a bindable target — the same reason
    /// <see cref="UpdateRelPathColumn"/> assigns its column too.</summary>
    private void UpdatePreviewPane()
    {
        var show = Tab.IsPreviewVisible;
        PreviewSplitterColumn.Width = show ? GridLength.Auto : new GridLength(0);
        PreviewColumn.Width = show ? new GridLength(PreviewWidth()) : new GridLength(0);
        PreviewSplitter.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        PreviewPane.Visibility = show ? Visibility.Visible : Visibility.Collapsed;

        if (show) Tab.Preview.Show(Tab.SelectedItems);
    }

    /// <summary>Clamped, because a width saved on a wide monitor must not leave a narrow pane with
    /// no file list at all.</summary>
    private double PreviewWidth() => Math.Clamp(_settings.PreviewPaneWidth, 180, 1200);

    private void PreviewSplitter_DragCompleted(object sender, DragCompletedEventArgs e)
    {
        // The dragged width is persisted the way PaneLayoutHost writes its splitter weights back:
        // on completion, not on every pixel of the drag.
        if (PreviewColumn.ActualWidth > 0)
            _settings.PreviewPaneWidth = PreviewColumn.ActualWidth;
    }

    public void FocusList() =>
        Dispatcher.InvokeAsync(() => FileListView.Focus(), DispatcherPriority.Input);

    public void FocusSearchBox()
    {
        SearchBox.Focus();
        SearchBox.SelectAll();
    }

    // --- View mode ---

    private bool? _thumbnailViewApplied;

    private void FileList_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(FileListViewModel.IsFlattened))
            UpdateRelPathColumn();
        else if (e.PropertyName == nameof(FileListViewModel.Items))
        {
            // Before the focus call: selecting scrolls the list, and doing it the other way round
            // would move the caret off whatever was just brought into view.
            TryApplyPendingSelection(clearIfMissing: true);
            FocusFileList();
        }
        else if (e.PropertyName == nameof(FileListViewModel.IsThumbnailView))
            ApplyViewMode();
    }

    /// <summary>Swaps the file list between the details <see cref="GridView"/> and the
    /// thumbnail-tile layout to match <see cref="FileListViewModel.IsThumbnailView"/>.
    /// One ListView is reused so all its interactions (selection, context menu, double-click,
    /// type-ahead) work in both modes.</summary>
    private void ApplyViewMode()
    {
        var thumbnails = Tab.FileList.IsThumbnailView;
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
    /// work without a click first.</summary>
    /// <remarks>Gated hard: with several tabs open a background directory finishing its load must
    /// never pull the caret out of whatever the user is typing in — including a search box in a
    /// different pane. Focus is only ever taken back by a list that already had it.</remarks>
    private void FocusFileList()
    {
        if (!Tab.IsActive || !IsKeyboardFocusWithin) return;
        if (Keyboard.FocusedElement is TextBoxBase) return;
        FocusList();
    }

    /// <summary>The Folder column only makes sense in the flattened search-results list.</summary>
    private void UpdateRelPathColumn() =>
        RelPathColumn.Width = Tab.FileList.IsFlattened ? 220 : 0;

    private void Scroll_PreviewMouseWheel(object sender, MouseWheelEventArgs e) =>
        ScrollSpeed.HandlePreviewMouseWheel(sender, e, _settings);

    // --- Breadcrumb / path box ---

    private void BreadcrumbSegment_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string path })
            _ = Tab.NavigateToAsync(path);
        e.Handled = true;
    }

    /// <summary>Middle-clicking a breadcrumb segment opens that ancestor in a pane of its own to
    /// the right, without leaving where you are.</summary>
    private void Breadcrumb_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Middle) return;
        if (VisualTreeUtil.FindAncestor<Button>(e.OriginalSource as DependencyObject)
            is not { Tag: string path }) return;

        _shell.OpenInNewPane(path, SplitOrientation.Vertical);
        e.Handled = true;
    }

    private void Breadcrumb_EmptyClick(object sender, MouseButtonEventArgs e)
    {
        PathBox.Text = Tab.CurrentPath;
        BreadcrumbScroller.Visibility = Visibility.Collapsed;
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
                _ = Tab.NavigateToAsync(path);
        }
        else if (e.Key == Key.Escape)
        {
            HidePathBox();
        }
    }

    private void PathBox_LostFocus(object sender, RoutedEventArgs e) => HidePathBox();

    private void HidePathBox()
    {
        PathBox.Visibility = Visibility.Collapsed;
        BreadcrumbScroller.Visibility = Visibility.Visible;
    }

    // --- Search box ---

    private void SearchBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Escape)
        {
            Tab.ClearSearchCommand.Execute(null);
            FileListView.Focus();
            e.Handled = true;
        }
    }

    // --- Keyboard ---

    protected override void OnPreviewKeyDown(KeyEventArgs e)
    {
        // Clipboard shortcuts belong to the list, not to a text box that happens to be in the
        // same pane — Ctrl+C in the search field must still copy text. (Ctrl+F is handled by the
        // window, so it reaches the active pane even from the sidebar.)
        // Ctrl+Shift+C, checked before plain Ctrl+C: the modifier comparison below is exact, so it
        // would not swallow this, but keeping the more specific gesture first is what stops the
        // next edit to that condition from doing so.
        if (FileListView.IsKeyboardFocusWithin &&
            Keyboard.Modifiers == (ModifierKeys.Control | ModifierKeys.Shift) && e.Key == Key.C)
        {
            _shell.CopyPathsCommand.Execute(SelectedFileItems());
            e.Handled = true;
        }
        else if (FileListView.IsKeyboardFocusWithin && Keyboard.Modifiers == ModifierKeys.Control &&
                 e.Key is Key.C or Key.X or Key.V)
        {
            // All three are off inside a container. Cut and paste would write; copy would make a
            // promise to produce bytes later that an entry cannot keep, since the archive may have
            // been rewritten by the time anything pastes. Still handled, so the chord is inert
            // rather than falling through to something else.
            if (!Tab.FileList.IsInsideArchive)
            {
                switch (e.Key)
                {
                    case Key.C: _shell.CopySelectionCommand.Execute(SelectedFileItems()); break;
                    case Key.X: _shell.CutSelectionCommand.Execute(SelectedFileItems()); break;
                    case Key.V: _shell.PasteCommand.Execute(null); break;
                }
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

    // --- File list interactions ---

    private void FileList_HeaderClick(object sender, RoutedEventArgs e)
    {
        if (e.OriginalSource is GridViewColumnHeader { Tag: string tag } &&
            Enum.TryParse<SortColumn>(tag, out var column))
        {
            Tab.FileList.SetSort(column);
        }
    }

    /// <summary>Updates the status-bar selection summary and mirrors a single selected *directory*
    /// into the folder tree. Selecting a file reveals nothing: its folder is already the one the tree
    /// is sitting on, so scrolling for it is pure churn. Multi-selection has no one folder to reveal,
    /// and a rubber-band drag churns the selection every frame, so both skip the tree work.</summary>
    private void FileList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        // Mirrored out synchronously so the shell and the window's key handlers never have to reach
        // into this view to find what is selected.
        Tab.SelectedItems = SelectedFileItems();
        QueueSelectionSummary();

        if (_marquee.IsDragging || FileListView.SelectedItems.Count != 1) return;
        if (FileListView.SelectedItem is not FileItemViewModel { IsDirectory: true } item) return;

        var dir = item.FullPath;
        if (string.IsNullOrEmpty(dir)) return;

        // Dropped by the shell unless this is the active tab, so a background pane can never move
        // the one shared folder tree.
        _shell.RequestTreeReveal(Tab, dir);
    }

    private bool _selectionSummaryPending;

    /// <summary>Coalesces the summary refresh to one per frame: a rubber-band drag adds and removes
    /// items one at a time, and each recount walks the whole selection. The preview rides the same
    /// coalescing, and is skipped outright while the rubber band is down — it would otherwise start
    /// a file read for every row the band swept over.</summary>
    private void QueueSelectionSummary()
    {
        if (_selectionSummaryPending) return;
        _selectionSummaryPending = true;
        _ = Dispatcher.InvokeAsync(() =>
        {
            _selectionSummaryPending = false;
            UpdateSelectionSummary();
            if (Tab.IsPreviewVisible && !_marquee.IsDragging)
                Tab.Preview.Show(Tab.SelectedItems);
        }, DispatcherPriority.Background);
    }

    /// <summary>Explorer-style "N items selected (size)" in the status bar. Folder sizes count
    /// only when they've already been computed, so the total never blocks on a scan.</summary>
    private void UpdateSelectionSummary()
    {
        var selected = FileListView.SelectedItems;
        if (selected.Count == 0)
        {
            Tab.SelectionSummary = "";
            return;
        }

        var bytes = 0L;
        foreach (var entry in selected)
        {
            if (entry is FileItemViewModel { SizeBytes: { } size })
                bytes += size;
        }

        var noun = selected.Count == 1 ? "item" : "items";
        Tab.SelectionSummary = bytes > 0
            ? $"{selected.Count:N0} {noun} selected ({ByteSizeFormatter.Format(bytes)})"
            : $"{selected.Count:N0} {noun} selected";
    }

    /// <summary>WPF doesn't select on right-click, so without this the context menu would act on a
    /// stale selection. Right-clicking inside the selection keeps it (that's how you get a menu for
    /// many items); right-clicking outside narrows it to the row under the cursor, like Explorer.</summary>
    private void FileList_PreviewMouseRightButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
            is not { DataContext: FileItemViewModel item }) return;
        if (FileListView.SelectedItems.Contains(item)) return;

        FileListView.SelectedItem = item;
    }

    /// <summary>Middle-clicking a folder opens it in a pane of its own to the right — the same
    /// thing the context menu's "Open in pane right" does.</summary>
    private void FileList_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Middle) return;
        if (VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
            is not { DataContext: FileItemViewModel { IsDirectory: true } folder }) return;

        _shell.OpenInNewPane(folder.FullPath, SplitOrientation.Vertical);
        e.Handled = true;
    }

    /// <summary>Explorer's convention: Ctrl+Shift turns an open into "run as administrator".</summary>
    private static bool RunAsAdminHeld =>
        Keyboard.Modifiers == (ModifierKeys.Control | ModifierKeys.Shift);

    private void FileList_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        // The row under the cursor, not the selection: Ctrl+Shift has already told an Extended
        // ListView to range-extend, so SelectedItem is the far end of that range rather than the
        // thing that was double-clicked.
        if (VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
            is { DataContext: FileItemViewModel clicked })
        {
            Tab.Open(clicked, RunAsAdminHeld);
            return;
        }

        if (FileListView.SelectedItem is FileItemViewModel item)
            Tab.Open(item, RunAsAdminHeld);
    }

    /// <summary>Enter opens the selected item, like double-click; Ctrl+Shift+Enter opens it as
    /// administrator. F2 renames the selection and Delete removes it, as everywhere else in
    /// Windows. Shift+Delete is the Explorer convention for erasing outright instead of setting
    /// aside.</summary>
    private void FileList_KeyDown(object sender, KeyEventArgs e)
    {
        // The plain-Enter arm needs its modifier guard or it swallows Ctrl+Shift+Enter first.
        if (e.Key == Key.Enter
            && Keyboard.Modifiers is ModifierKeys.None or (ModifierKeys.Control | ModifierKeys.Shift)
            && FileListView.SelectedItem is FileItemViewModel item)
        {
            Tab.Open(item, RunAsAdminHeld);
            e.Handled = true;
        }
        // Each of the three write verbs carries the archive guard as well as its modifier guard.
        // The menu hides them there too, but a keybinding must not be able to route around a menu —
        // and each still marks the key handled, so the chord does nothing rather than falling
        // through to type-ahead and jumping the selection to a file beginning with "n".
        else if (e.Key == Key.F2 && Keyboard.Modifiers == ModifierKeys.None)
        {
            _ = RenameSelectionAsync();
            e.Handled = true;
        }
        else if (e.Key == Key.Delete && Keyboard.Modifiers is ModifierKeys.None or ModifierKeys.Shift)
        {
            // Shift is ignored inside a container, for the reason the menu item is hidden there.
            _ = DeleteSelectionAsync(
                permanent: Keyboard.Modifiers == ModifierKeys.Shift && !Tab.FileList.IsInsideArchive);
            e.Handled = true;
        }
        else if (e.Key == Key.N && Keyboard.Modifiers == (ModifierKeys.Control | ModifierKeys.Shift)
                 && !Tab.FileList.IsInsideArchive)
        {
            // Explorer's shortcut. It belongs on the list rather than in the window's InputBindings
            // because it acts on the focused pane's directory, and it needs its modifier guard
            // like every other arm here.
            _ = CreateInCurrentFolderAsync(NewItemKind.Folder);
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

    // --- Context menu ---

    private List<FileItemViewModel> SelectedFileItems() =>
        FileListView.SelectedItems.Cast<FileItemViewModel>().ToList();

    /// <summary>Enables clipboard items for the current state and rebuilds the
    /// user-defined command entries for the selection.</summary>
    private void FileList_ContextMenuOpening(object sender, ContextMenuEventArgs e)
    {
        if (FileListView.ContextMenu is not { } menu) return;

        var selection = SelectedFileItems();

        // Inside an archive nothing has a path an executor can act on, so everything that writes to
        // disk by path is off. The same guard the flattened-search rules use, for the same reason,
        // and the dangerous ones are guarded at the service too — a keybinding must not be able to
        // route around a menu.
        //
        // Copy is off with them, and that one is worth stating: Ctrl+C is a promise to produce
        // bytes *later*, and an entry cannot keep it — the container may have been rewritten by the
        // time anyone pastes. Explorer keeps the promise by extracting to temp on the keystroke,
        // which writes gigabytes for a keypress. Extract is the honest verb and it gets its own
        // menu item.
        var inArchive = Tab.FileList.IsInsideArchive;

        CopyMenuItem.IsEnabled = CutMenuItem.IsEnabled = selection.Count > 0 && !inArchive;
        CopyPathMenuItem.IsEnabled = CopyNameMenuItem.IsEnabled = selection.Count > 0;
        CopyPathMenuItem.Header = selection.Count > 1 ? "Copy as paths" : "Copy as path";
        CopyNameMenuItem.Header = selection.Count > 1 ? "Copy names" : "Copy name";
        PasteMenuItem.IsEnabled = FileClipboard.HasFiles() && !inArchive;

        // "Open in new tab/pane" only makes sense for folders.
        var folders = selection.Count(i => i.IsDirectory);
        OpenInNewTabMenuItem.IsEnabled = OpenInNewPaneMenuItem.IsEnabled = folders > 0;
        OpenInNewTabMenuItem.Header = folders > 1 ? $"Open {folders} folders in new tabs" : "Open in new tab";

        // One folder, since the view analyses a single root. With nothing selected this still
        // offers itself and analyses the folder being shown, which is the useful reading of an
        // empty-space right-click.
        DiskUsageMenuItem.IsEnabled = selection.Count == 0 || (selection.Count == 1 && folders == 1);
        // Duplicates reads whole files by path, which an entry does not have. Disk usage does not:
        // every size inside a container is already exact, so it stays on — see the Archives section.
        DuplicatesMenuItem.IsEnabled = DiskUsageMenuItem.IsEnabled && !inArchive;

        // Extract shows from either side of the container: on a single selected archive out here,
        // or on the selection (or everything, with nothing selected) in there. It is the one write
        // verb that is *more* available inside an archive than outside one.
        var extractable = inArchive ||
            (selection.Count == 1 && !selection[0].IsDirectory &&
             ArchiveFormats.IsArchiveName(selection[0].Name));

        ExtractHereMenuItem.Visibility = ExtractToMenuItem.Visibility =
            extractable ? Visibility.Visible : Visibility.Collapsed;

        // Compressing reads files by path, so it needs real ones — off inside a container and off
        // over a flattened search result, where "the folder being shown" is not a folder.
        CompressMenuItem.IsEnabled =
            !inArchive && !Tab.FileList.IsFlattened && Tab.CurrentPath.Length > 0;
        CompressMenuItem.Header = selection.Count > 1
            ? $"Compress {selection.Count:N0} items…"
            : "Compress…";
        ExtractHereMenuItem.Header = inArchive && selection.Count > 0
            ? $"Extract {selection.Count:N0} item(s) here"
            : "Extract here";

        // Only ever one file: "run this folder as administrator" means nothing, and a whole
        // selection of programs started elevated at once is not something to offer from a menu.
        RunAsAdminMenuItem.IsEnabled =
            selection.Count == 1 && !selection[0].IsDirectory && !inArchive;

        // Rename and Delete stay on inside a container, but they mean something different in
        // there: the container is rewritten beside itself and swapped in. Whether that is possible
        // at all depends on the format and the archive's own shape, and the planner is what knows —
        // so the menu offers it and the refusal, when there is one, arrives by name.
        //
        // Renaming several at once is not offered in there: a rewrite writes each entry exactly
        // once, so the staging trick that makes a rotating batch work on disk has nowhere to happen.
        RenameMenuItem.IsEnabled = selection.Count == 1 || (selection.Count > 1 && !inArchive);
        RenameMenuItem.Header = selection.Count > 1 ? $"Rename {selection.Count} items…" : "Rename…";

        DeleteMenuItem.IsEnabled = selection.Count > 0;

        // Shift+Delete has no meaning in there: an entry has no Recycle Bin and no staging of its
        // own, so there is no second, more destructive thing for it to mean.
        DeletePermanentlyMenuItem.IsEnabled = selection.Count > 0 && !inArchive;
        DeleteMenuItem.Header = selection.Count > 1 ? $"Delete {selection.Count} items…" : "Delete…";

        // Bookmarking is refused at BookmarkService too, and that is the important half: a virtual
        // path in the bookmark table would sort strictly inside the archive's own containing folder
        // under PathKey.IsUnder, and poison every subtree query over it.
        BookmarkMenuItem.IsEnabled = selection.Count > 0 && !inArchive;
        // "Remove bookmark" only when every selected item is already bookmarked.
        var allBookmarked = selection.Count > 0 && selection.All(i => _shell.Bookmarks.IsBookmarked(i.FullPath));
        BookmarkMenuItem.Header = allBookmarked ? "Remove bookmark" : "Bookmark";

        // New acts on the folder being shown, so it needs one — and a flattened search result is
        // not one: creating into the search root would produce an item that may not match the query
        // and so would not appear, which reads as a failure.
        NewMenuItem.IsEnabled =
            !Tab.FileList.IsFlattened && !inArchive && Tab.CurrentPath.Length > 0;
        NewItemMenu.Rebuild(NewMenuItem, NewFileTypesSeparator, _settings,
            template => _ = CreateInCurrentFolderAsync(NewItemKind.File, template));

        CustomCommandMenu.Rebuild(menu, CustomCommandsSeparator,
            selection.Select(i => (i.FullPath, i.IsDirectory)).ToList(),
            _settings, _shell.RunCustomCommand);
    }

    private void ContextNewFolder_Click(object sender, RoutedEventArgs e) =>
        _ = CreateInCurrentFolderAsync(NewItemKind.Folder);

    private void ContextNewEmptyFile_Click(object sender, RoutedEventArgs e) =>
        _ = CreateInCurrentFolderAsync(NewItemKind.File);

    /// <summary>Creates a folder or file in the directory this tab is showing. The selection is
    /// deliberately not consulted: New makes something beside what is here, never inside it.</summary>
    private async Task CreateInCurrentFolderAsync(
        NewItemKind kind, NewFileTemplate? template = null)
    {
        if (Tab.FileList.IsFlattened || Tab.CurrentPath.Length == 0) return;

        var directory = Tab.CurrentPath;
        var owner = Window.GetWindow(this);
        var suggestion = _shell.SuggestNewItemName(directory, kind, template);

        if (NewItemDialog.Show(owner, directory, kind, template, suggestion, _shell.PlanNewItem)
            is not { } plan)
        {
            return;
        }

        // The shell selects the new item through PendingSelection, so there is nothing to do here
        // on success — it reaches whichever pane is showing this folder, not just this one.
        var outcome = await _shell.CreateNewItemAsync(plan);

        if (outcome.Failed is { } failed)
            MessageDialog.Show(owner, failed.Message, "New", MessageDialogKind.Warning);
    }

    private void ContextOpen_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            Tab.OpenItemCommand.Execute(item);
    }

    private void ContextRunAsAdmin_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            Tab.Open(item, elevated: true);
    }

    /// <summary>Opening a whole selection of folders is capped: a stray Ctrl+A would otherwise
    /// turn one menu click into hundreds of directory loads.</summary>
    private const int MaxFoldersOpenedAtOnce = 10;

    private void ContextOpenInNewTab_Click(object sender, RoutedEventArgs e)
    {
        foreach (var folder in SelectedFolders())
            _shell.OpenInNewTab(folder.FullPath);
    }

    private void ContextOpenInPaneRight_Click(object sender, RoutedEventArgs e) =>
        OpenSelectedFolderInPane(SplitOrientation.Vertical);

    private void ContextOpenInPaneBelow_Click(object sender, RoutedEventArgs e) =>
        OpenSelectedFolderInPane(SplitOrientation.Horizontal);

    private void OpenSelectedFolderInPane(SplitOrientation orientation)
    {
        if (SelectedFolders().FirstOrDefault() is { } folder)
            _shell.OpenInNewPane(folder.FullPath, orientation);
    }

    private List<FileItemViewModel> SelectedFolders() =>
        SelectedFileItems().Where(i => i.IsDirectory).Take(MaxFoldersOpenedAtOnce).ToList();

    private void ContextOpenTerminal_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenInTerminal(item.FullPath, item.IsDirectory);
    }

    /// <summary>A selected folder, or — with nothing selected — the folder being shown.</summary>
    private void ContextDiskUsage_Click(object sender, RoutedEventArgs e)
    {
        var target = FileListView.SelectedItem is FileItemViewModel { IsDirectory: true } item
            ? item.FullPath
            : Tab.CurrentPath;

        _shell.OpenDiskUsage(target is { Length: > 0 } ? target : null);
    }

    /// <summary>A selected folder, or — with nothing selected — the folder being shown.</summary>
    /// <summary>
    /// Extract, from either side of the container: a selected archive out in the folder that holds
    /// it, or the selection inside one out to somewhere chosen.
    /// </summary>
    /// <remarks>
    /// Both go through <see cref="ShellViewModel.PlanExtract"/> and
    /// <see cref="ShellViewModel.ExecuteExtractAsync"/>, so there is one path to audit and one
    /// progress surface. Nothing here writes a file itself.
    /// </remarks>
    /// <summary>
    /// Asks for the current archive's password and reloads with it.
    /// </summary>
    /// <remarks>
    /// The reload is an ordinary refresh: the password store is consulted by the reader on the way
    /// through, and the cache treats an index read <em>with</em> a password as superseding one read
    /// without — so there is no special "unlocked" path, only a re-read that now succeeds.
    /// </remarks>
    private void Unlock_Click(object sender, RoutedEventArgs e) => _ = UnlockAsync();

    private async Task UnlockAsync()
    {
        if (_shell.ArchiveFileFor(Tab.CurrentPath) is not { } archive) return;

        var dialog = ArchivePasswordDialog.Create(
            Window.GetWindow(this), Path.GetFileName(archive), retry: _unlockRefused);

        if (dialog.ShowDialog() != true) return;

        _shell.RememberArchivePassword(archive, dialog.Password);
        await Tab.RefreshViewAsync();

        // A wrong password comes straight back as the same locked state, so the next attempt says
        // so rather than repeating the first, more optimistic, wording.
        _unlockRefused = Tab.FileList.IsArchiveLocked;
        if (!_unlockRefused) return;

        _shell.ForgetArchivePassword(archive);
    }

    /// <summary>Whether the last password given for this tab's archive was refused.</summary>
    private bool _unlockRefused;

    /// <summary>Compress the selection, or the whole folder when nothing is selected.</summary>
    private void ContextCompress_Click(object sender, RoutedEventArgs e) => _ = CompressAsync();

    private async Task CompressAsync()
    {
        var selection = SelectedFileItems();
        var sources = selection.Count > 0
            ? selection.Select(i => i.FullPath).ToList()
            : [Tab.CurrentPath];

        var directory = Path.GetDirectoryName(Tab.CurrentPath.TrimEnd(Path.DirectorySeparatorChar)) is { Length: > 0 } above
                        && selection.Count == 0
            ? above                      // compressing the folder itself puts the archive beside it
            : Tab.CurrentPath;

        var dialog = CreateArchiveDialog.Create(
            Window.GetWindow(this), directory,
            ArchiveWriteRules.SuggestName(sources, Tab.CurrentPath), sources.Count);

        if (dialog.ShowDialog() != true) return;

        await _shell.ExecuteCreateArchiveAsync(
            sources, dialog.ArchivePath, dialog.Format, dialog.Level);
    }

    private void ContextExtractHere_Click(object sender, RoutedEventArgs e) =>
        _ = ExtractAsync(askWhere: false);

    private void ContextExtractTo_Click(object sender, RoutedEventArgs e) =>
        _ = ExtractAsync(askWhere: true);

    private async Task ExtractAsync(bool askWhere)
    {
        var (archive, entries) = ExtractTarget();
        if (archive is null) return;

        var destination = _shell.SuggestExtractDestination(archive);
        var conflict = ExtractConflict.KeepBoth;

        if (askWhere)
        {
            var dialog = ExtractDialog.Create(
                Window.GetWindow(this), Path.GetFileName(archive), destination, entries.Count);
            if (dialog.ShowDialog() != true) return;

            destination = dialog.Destination;
            conflict = dialog.Conflict;
        }

        // Entering the container first is what makes "Extract here" work on a row in the folder
        // above it: the plan is expressed in terms of the archive the tab is showing.
        if (!Tab.FileList.IsInsideArchive)
        {
            await Tab.NavigateToAsync(archive);
            entries = [];
        }

        var plan = _shell.PlanExtract(Tab, entries, destination, conflict);
        if (plan.Rejected is { } rejected)
        {
            MessageDialog.Show(
                Window.GetWindow(this), rejected.Message, "Extract", MessageDialogKind.Warning);
            return;
        }

        await _shell.ExecuteExtractAsync(plan);
    }

    /// <summary>
    /// What to extract: the selected archive when standing outside one, or the selection (or
    /// everything) when standing inside one.
    /// </summary>
    private (string? Archive, IReadOnlyList<string> Entries) ExtractTarget()
    {
        var selection = SelectedFileItems();

        if (Tab.FileList.IsInsideArchive)
            return (Tab.CurrentPath, selection.Select(i => i.FullPath).ToList());

        var one = selection.Count == 1 && !selection[0].IsDirectory ? selection[0] : null;
        return one is not null && ArchiveFormats.IsArchiveName(one.Name)
            ? (one.FullPath, [])
            : (null, []);
    }

    private void ContextDuplicates_Click(object sender, RoutedEventArgs e)
    {
        var target = FileListView.SelectedItem is FileItemViewModel { IsDirectory: true } item
            ? item.FullPath
            : Tab.CurrentPath;

        _shell.OpenDuplicates(target is { Length: > 0 } ? target : null);
    }

    private void ContextOpenVSCode_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenInVSCode(item.FullPath, item.IsDirectory);
    }

    private void ContextCopy_Click(object sender, RoutedEventArgs e) =>
        _shell.CopySelectionCommand.Execute(SelectedFileItems());

    private void ContextCut_Click(object sender, RoutedEventArgs e) =>
        _shell.CutSelectionCommand.Execute(SelectedFileItems());

    private void ContextPaste_Click(object sender, RoutedEventArgs e) =>
        _shell.PasteCommand.Execute(null);

    private void ContextCopyPath_Click(object sender, RoutedEventArgs e) =>
        _shell.CopyPathsCommand.Execute(SelectedFileItems());

    private void ContextCopyName_Click(object sender, RoutedEventArgs e) =>
        _shell.CopyNamesCommand.Execute(SelectedFileItems());

    private void ContextRename_Click(object sender, RoutedEventArgs e) => _ = RenameSelectionAsync();

    /// <summary>Renames the selection, in the order the list shows it — which is the order a
    /// numbered rename counts in, so "Holiday 1" is the one nearest the top.</summary>
    private async Task RenameSelectionAsync()
    {
        var selection = SelectedFileItems();
        if (selection.Count == 0) return;

        if (Tab.FileList.IsInsideArchive)
        {
            await RenameInArchiveAsync(selection[0]);
            return;
        }

        // SelectedItems is in the order things were clicked, which is not the order they are read in.
        // The date goes across local, matching the Modified column the user was just reading, and
        // stays null when the row has never been hydrated — a search result arrives that way, and
        // {modified} refuses such an item rather than stamping it year one.
        var ordered = Tab.FileList.Items
            .Where(selection.Contains)
            .Select(i => new RenameSource(i.FullPath, i.IsDirectory,
                i.ModifiedUtc == default ? null : i.ModifiedUtc.ToLocalTime()))
            .ToList();

        var owner = Window.GetWindow(this);
        if (RenameDialog.Show(owner, ordered, _shell.PlanRename) is not { } plan) return;

        var outcome = await _shell.RenameAsync(plan);

        // The shell has reloaded this list by now, so the rows are new objects: re-select what was
        // renamed rather than leaving the user with nothing selected where their files used to be.
        SelectPaths(outcome.Completed.Select(c => c.FinalPath));

        if (outcome.Failed.Count == 0) return;

        MessageDialog.Show(owner, string.Join("\n\n", outcome.Failed.Select(f => f.Message)),
            "Rename", MessageDialogKind.Warning);
    }

    /// <summary>
    /// Highlights whatever a <c>/select</c> request asked for, as soon as the row it names exists.
    /// </summary>
    /// <param name="clearIfMissing">
    /// True when the listing has just been replaced — the load this selection was waiting for has
    /// happened, so a row that is still not there is never coming and the request is spent. False
    /// when only the request itself arrived: the rows on screen may still belong to the previous
    /// folder, and giving up on them would lose the selection the navigation is about to make
    /// possible.
    /// </param>
    /// <remarks>
    /// Deferred, and that is load-bearing. Both signals arrive <em>before</em> the
    /// <see cref="ListView"/>'s <c>ItemsSource</c> binding has caught up with the view model's new
    /// collection — WPF updates bindings at <see cref="DispatcherPriority.DataBind"/> — so selecting
    /// straight away searches the folder that was showing a moment ago and silently finds nothing.
    /// </remarks>
    private void TryApplyPendingSelection(bool clearIfMissing)
    {
        if (Tab.PendingSelection is not { Length: > 0 }) return;

        _ = Dispatcher.InvokeAsync(
            () =>
            {
                if (Tab.PendingSelection is not { Length: > 0 } path) return;
                if (SelectPaths([path]) || clearIfMissing) Tab.PendingSelection = null;
            },
            DispatcherPriority.Background);
    }

    /// <summary>Selects the given paths. False when none of them are in the list.</summary>
    private bool SelectPaths(IEnumerable<string> paths)
    {
        var wanted = paths.ToHashSet(StringComparer.OrdinalIgnoreCase);
        if (wanted.Count == 0) return false;

        FileListView.SelectedItems.Clear();
        foreach (var obj in FileListView.Items)
        {
            if (obj is FileItemViewModel vm && wanted.Contains(vm.FullPath))
                FileListView.SelectedItems.Add(vm);
        }
        if (FileListView.SelectedItem is not { } first) return false;

        FileListView.ScrollIntoView(first);
        return true;
    }

    private void ContextDelete_Click(object sender, RoutedEventArgs e) =>
        _ = DeleteSelectionAsync(permanent: false);

    private void ContextDeletePermanently_Click(object sender, RoutedEventArgs e) =>
        _ = DeleteSelectionAsync(permanent: true);

    /// <summary>
    /// Renaming one entry inside a container, which means rewriting the whole container.
    /// </summary>
    /// <remarks>
    /// The same dialog, so the naming rules the user knows are the naming rules that apply — but
    /// one item only, because a rewrite writes each entry exactly once and the staging trick that
    /// makes a rotating batch work on disk has nowhere to happen in here.
    /// </remarks>
    private async Task RenameInArchiveAsync(FileItemViewModel item)
    {
        var owner = Window.GetWindow(this);

        var sources = new List<RenameSource>
        {
            new(item.FullPath, item.IsDirectory,
                item.ModifiedUtc == default ? null : item.ModifiedUtc.ToLocalTime()),
        };

        if (RenameDialog.Show(owner, sources, _shell.PlanRenameInArchive) is not { } plan) return;
        if (plan.Renames.Count == 0) return;

        var edits = new List<ArchiveEdit>
        {
            new RenameEntry(
                _shell.ArchiveEntryPathFor(item.FullPath) ?? "",
                plan.Renames[0].TargetName),
        };

        await RunArchiveEditAsync(edits, owner);
    }

    /// <summary>
    /// Plans and runs an archive edit, reporting a refusal rather than appearing to do nothing.
    /// </summary>
    private async Task RunArchiveEditAsync(IReadOnlyList<ArchiveEdit> edits, Window? owner)
    {
        var plan = _shell.PlanArchiveEdit(Tab, edits);

        if (plan.Rejected is { } rejected)
        {
            MessageDialog.Show(owner, rejected.Message, "Archive", MessageDialogKind.Warning);
            return;
        }

        await _shell.ExecuteArchiveEditAsync(plan);
    }

    /// <summary>Deletes the selection, after showing exactly what that comes to.</summary>
    /// <remarks>The plan is built before the confirmation and handed straight to the executor, so
    /// the items the user was shown are the items that go — and the executor re-checks every one of
    /// them against disk, because the dialog may have sat open for a while.</remarks>
    private async Task DeleteSelectionAsync(bool permanent)
    {
        var selection = SelectedFileItems();
        if (selection.Count == 0) return;

        if (Tab.FileList.IsInsideArchive)
        {
            // No planner, no survey and no Recycle Bin: an entry has none of those. The container
            // is rewritten without it, and the whole original is what Ctrl+Z puts back.
            var owner2 = Window.GetWindow(this);
            var what = selection.Count == 1
                ? selection[0].Name
                : $"{selection.Count:N0} items";
            var confirmed = MessageDialog.Show(
                owner2,
                $"Remove {what} from this archive?\n\n" +
                "The whole archive is rewritten to do it. Ctrl+Z puts the original back.",
                "Archive", MessageDialogKind.Warning, showCancel: true);

            if (confirmed)
                await RunArchiveEditAsync(
                    _shell.RemovalsFor(selection.Select(i => i.FullPath).ToList()), owner2);
            return;
        }

        // In the order the list shows them, so the confirmation reads down the screen.
        var ordered = Tab.FileList.Items
            .Where(selection.Contains)
            .Select(i => new DeleteSource(i.FullPath, i.IsDirectory))
            .ToList();

        var owner = Window.GetWindow(this);
        // Delete goes to the Windows Recycle Bin; the planner falls back to this app's own holding
        // folder per item where a volume has no bin. Shift+Delete still erases outright.
        var plan = _shell.PlanDelete(ordered, permanent ? DeleteMode.Permanent : DeleteMode.Recycle);

        if (!plan.HasWork)
        {
            // Nothing survived the rules — say why rather than appearing to do nothing.
            if (plan.Problems is { Count: > 0 } problems)
                MessageDialog.Show(owner, string.Join("\n\n", problems.Select(p => p.Message)),
                    "Delete", MessageDialogKind.Warning);
            return;
        }

        if (!DeleteDialog.Confirm(owner, plan, _shell.SurveyDelete)) return;

        var outcome = await _shell.DeleteAsync(plan);
        if (outcome.Failed.Count == 0) return;

        MessageDialog.Show(owner, string.Join("\n\n", outcome.Failed.Select(f => f.Message)),
            "Delete", MessageDialogKind.Warning);
    }

    private void ContextBookmark_Click(object sender, RoutedEventArgs e)
    {
        var entries = SelectedFileItems().Select(i => (i.FullPath, i.IsDirectory)).ToList();
        _ = _shell.ToggleBookmarksAsync(entries);
    }

    private void ContextProperties_Click(object sender, RoutedEventArgs e)
    {
        if (SelectedFileItems() is { Count: > 0 } selection)
            ShowProperties(selection);
    }

    private void ShowProperties(IReadOnlyList<FileItemViewModel> items)
    {
        if (PropertiesPrompt.Show(items))
            Tab.RefreshCommand.Execute(null); // hidden-bit toggles can add/remove rows
    }
}
