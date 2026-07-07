using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

public partial class MainWindow : Window
{
    private readonly ShellViewModel _shell;
    private readonly BertBrowser.App.Services.AppSettings _settings;

    public MainWindow(ShellViewModel shell, BertBrowser.App.Services.AppSettings settings)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        DataContext = shell;

        ApplyWindowSettings();

        _shell.FileList.PropertyChanged += FileList_PropertyChanged;
        _shell.RevealFileRequested += OnRevealFileRequested;
        UpdateRelPathColumn();
        ApplyViewMode(); // honor a restored thumbnail zoom level

        Loaded += async (_, _) => await _shell.InitializeAsync();
        Closing += (_, _) => SaveWindowSettings();
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
        _settings.LastPath = _shell.CurrentPath.Length > 0 ? _shell.CurrentPath : null;
        _settings.Save(); // per-directory thumbnail scales are already updated live in the map
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
            FileListView.View = null; // a null View lets ItemsPanel/ItemTemplate take over
            FileListView.ItemsPanel = (ItemsPanelTemplate)FindResource("ThumbPanel");
            FileListView.ItemTemplate = (DataTemplate)FindResource("ThumbTileTemplate");
            FileListView.ItemContainerStyle = (Style)FindResource("ThumbItemStyle");
            // Disabling the horizontal scrollbar bounds the WrapPanel to the viewport width so
            // tiles roll onto the next row; only a vertical scrollbar ever appears.
            ScrollViewer.SetHorizontalScrollBarVisibility(FileListView, ScrollBarVisibility.Disabled);
        }
        else
        {
            FileListView.View = DetailsView;
            FileListView.ClearValue(ItemsControl.ItemsPanelProperty);   // restore virtualizing stack
            FileListView.ClearValue(ItemsControl.ItemTemplateProperty); // GridView supplies cells
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

    /// <summary>The Relative path column only makes sense in flattened tag-filter mode.</summary>
    private void UpdateRelPathColumn() =>
        RelPathColumn.Width = _shell.FileList.IsFlattened ? 220 : 0;

    // --- Toolbar / dialogs ---

    private async void ManageTags_Click(object sender, RoutedEventArgs e)
    {
        var vm = new TagManagerViewModel(App.Services.GetRequiredService<ITagService>());
        var window = new TagManagerWindow(vm) { Owner = this };
        window.ShowDialog();
        await _shell.OnTagsChangedAsync();
    }

    private void Settings_Click(object sender, RoutedEventArgs e)
    {
        var vm = new SettingsViewModel(_settings);
        if (new SettingsWindow(vm) { Owner = this }.ShowDialog() == true)
        {
            // Reload so a changed "Show hidden items" setting takes effect immediately.
            // (Custom-command menus rebuild on every open, so they need no refresh.)
            _shell.RefreshCommand.Execute(null);
        }
    }

    private void ScanProgress_Click(object sender, RoutedEventArgs e) =>
        new ScanProgressWindow(_shell) { Owner = this }.ShowDialog();

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
                 FileListView.SelectedItem is FileItemViewModel selected)
        {
            ShowProperties(selected.FullPath, selected.IsDirectory);
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

    /// <summary>Mirrors the main-panel selection into the folder tree (the item's own
    /// folder for directories, its parent for files), expanding and scrolling as needed.</summary>
    private void FileList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (FileListView.SelectedItem is not FileItemViewModel item) return;

        var dir = item.IsDirectory ? item.FullPath : Path.GetDirectoryName(item.FullPath);
        if (string.IsNullOrEmpty(dir)) return;

        var chain = _shell.Tree.RevealPath(dir);
        if (chain.Count == 0) return;

        // Containers for freshly expanded nodes only exist after a layout pass.
        Dispatcher.InvokeAsync(() => ScrollTreeChainIntoView(chain), DispatcherPriority.Loaded);
    }

    /// <summary>Positions the revealed node roughly 40% down the tree's viewport.</summary>
    private void ScrollTreeChainIntoView(IReadOnlyList<DirectoryNodeViewModel> chain)
    {
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

    private void FileList_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenItemCommand.Execute(item);
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

    private async void ContextTags_Click(object sender, RoutedEventArgs e)
    {
        var paths = FileListView.SelectedItems.Cast<FileItemViewModel>()
            .Where(i => !i.IsDirectory)
            .Select(i => i.FullPath)
            .ToList();
        if (paths.Count == 0)
        {
            _shell.StatusText = "Select one or more files to tag (folders cannot be tagged).";
            return;
        }

        var vm = new AssignTagsViewModel(App.Services.GetRequiredService<ITagService>(), paths);
        var dialog = new AssignTagsDialog(vm) { Owner = this };
        if (dialog.ShowDialog() == true)
            await _shell.OnTagsChangedAsync();
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

    private void ContextRemoveMissing_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.RemoveMissingCommand.Execute(item);
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
        if (FileListView.SelectedItem is FileItemViewModel item)
            ShowProperties(item.FullPath, item.IsDirectory);
    }

    private void ShowProperties(string fullPath, bool isDirectory)
    {
        var vm = new PropertiesViewModel(fullPath, isDirectory,
            App.Services.GetRequiredService<ITagService>(),
            App.Services.GetRequiredService<IDirectorySizeService>(),
            App.Services.GetRequiredService<DirSizeRepository>());
        new PropertiesDialog(vm) { Owner = this }.ShowDialog();
        if (vm.AttributesChanged)
            _shell.RefreshCommand.Execute(null); // hidden-bit toggles can add/remove rows
    }
}
