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
        // Attached after the marquee so the two never fight: the marquee ignores presses that land
        // on a row, and this ignores presses that land on empty space.
        FileDragDropController.Attach(FileListView, tab, shell);

        Tab.FileList.PropertyChanged += FileList_PropertyChanged;
        Tab.PropertyChanged += Tab_PropertyChanged;
        Tab.RevealFileRequested += OnRevealFileRequested;
        UpdateRelPathColumn();
        ApplyViewMode(); // honor a restored thumbnail zoom level
    }

    /// <summary>Gives the subscriptions back. A tab is closable, unlike the window, so its view
    /// has to let go of the view model when it goes.</summary>
    public void Detach()
    {
        Tab.FileList.PropertyChanged -= FileList_PropertyChanged;
        Tab.PropertyChanged -= Tab_PropertyChanged;
        Tab.RevealFileRequested -= OnRevealFileRequested;
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

        if (e.PropertyName != nameof(DirectoryTabViewModel.CurrentPath)) return;
        _ = Dispatcher.InvokeAsync(BreadcrumbScroller.ScrollToRightEnd, DispatcherPriority.Loaded);
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

    /// <summary>Middle-clicking a breadcrumb segment opens that ancestor in a background tab,
    /// without leaving where you are.</summary>
    private void Breadcrumb_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Middle) return;
        if (VisualTreeUtil.FindAncestor<Button>(e.OriginalSource as DependencyObject)
            is not { Tag: string path }) return;

        _shell.OpenInNewTab(path);
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

    /// <summary>Middle-clicking a folder opens it in a background tab, as in a browser.</summary>
    private void FileList_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Middle) return;
        if (VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
            is not { DataContext: FileItemViewModel { IsDirectory: true } folder }) return;

        _shell.OpenInNewTab(folder.FullPath);
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
        else if (e.Key == Key.F2 && Keyboard.Modifiers == ModifierKeys.None)
        {
            _ = RenameSelectionAsync();
            e.Handled = true;
        }
        else if (e.Key == Key.Delete && Keyboard.Modifiers is ModifierKeys.None or ModifierKeys.Shift)
        {
            _ = DeleteSelectionAsync(permanent: Keyboard.Modifiers == ModifierKeys.Shift);
            e.Handled = true;
        }
        else if (e.Key == Key.N && Keyboard.Modifiers == (ModifierKeys.Control | ModifierKeys.Shift))
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
        CopyMenuItem.IsEnabled = CutMenuItem.IsEnabled = selection.Count > 0;
        CopyPathMenuItem.IsEnabled = CopyNameMenuItem.IsEnabled = selection.Count > 0;
        CopyPathMenuItem.Header = selection.Count > 1 ? "Copy as paths" : "Copy as path";
        CopyNameMenuItem.Header = selection.Count > 1 ? "Copy names" : "Copy name";
        PasteMenuItem.IsEnabled = FileClipboard.HasFiles();

        // "Open in new tab/pane" only makes sense for folders.
        var folders = selection.Count(i => i.IsDirectory);
        OpenInNewTabMenuItem.IsEnabled = OpenInNewPaneMenuItem.IsEnabled = folders > 0;
        OpenInNewTabMenuItem.Header = folders > 1 ? $"Open {folders} folders in new tabs" : "Open in new tab";

        // One folder, since the view analyses a single root. With nothing selected this still
        // offers itself and analyses the folder being shown, which is the useful reading of an
        // empty-space right-click.
        DiskUsageMenuItem.IsEnabled = selection.Count == 0 || (selection.Count == 1 && folders == 1);

        // Only ever one file: "run this folder as administrator" means nothing, and a whole
        // selection of programs started elevated at once is not something to offer from a menu.
        RunAsAdminMenuItem.IsEnabled =
            selection.Count == 1 && !selection[0].IsDirectory;

        RenameMenuItem.IsEnabled = selection.Count > 0;
        RenameMenuItem.Header = selection.Count > 1 ? $"Rename {selection.Count} items…" : "Rename…";

        DeleteMenuItem.IsEnabled = DeletePermanentlyMenuItem.IsEnabled = selection.Count > 0;
        DeleteMenuItem.Header = selection.Count > 1 ? $"Delete {selection.Count} items…" : "Delete…";

        BookmarkMenuItem.IsEnabled = selection.Count > 0;
        // "Remove bookmark" only when every selected item is already bookmarked.
        var allBookmarked = selection.Count > 0 && selection.All(i => _shell.Bookmarks.IsBookmarked(i.FullPath));
        BookmarkMenuItem.Header = allBookmarked ? "Remove bookmark" : "Bookmark";

        // New acts on the folder being shown, so it needs one — and a flattened search result is
        // not one: creating into the search root would produce an item that may not match the query
        // and so would not appear, which reads as a failure.
        NewMenuItem.IsEnabled = !Tab.FileList.IsFlattened && Tab.CurrentPath.Length > 0;
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

        // SelectedItems is in the order things were clicked, which is not the order they are read in.
        var ordered = Tab.FileList.Items
            .Where(selection.Contains)
            .Select(i => new RenameSource(i.FullPath, i.IsDirectory))
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

    /// <summary>Deletes the selection, after showing exactly what that comes to.</summary>
    /// <remarks>The plan is built before the confirmation and handed straight to the executor, so
    /// the items the user was shown are the items that go — and the executor re-checks every one of
    /// them against disk, because the dialog may have sat open for a while.</remarks>
    private async Task DeleteSelectionAsync(bool permanent)
    {
        var selection = SelectedFileItems();
        if (selection.Count == 0) return;

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
