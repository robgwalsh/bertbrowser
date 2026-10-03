using System.ComponentModel;
using BertBrowser.App.Theming;
using Microsoft.Extensions.DependencyInjection;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Services.Duplicates;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Views;

public partial class MainWindow : ThemedWindow
{
    private readonly ShellViewModel _shell;
    private readonly BertBrowser.App.Services.AppSettings _settings;
    private readonly PaneLayoutHost _layoutHost;

    /// <summary>The modeless disk-usage window while one is open, so a second request re-points
    /// it instead of stacking another.</summary>
    private DiskUsageWindow? _diskUsage;

    /// <summary>The modeless duplicates window while one is open, re-pointed rather than stacked.</summary>
    private DuplicatesWindow? _duplicates;

    /// <summary>The modeless change timeline while one is open, re-pointed rather than stacked.</summary>
    private ChangeTimelineWindow? _changes;

    /// <summary>The modeless checksum window while one is open, re-pointed rather than stacked.</summary>
    private ChecksumWindow? _checksums;

    /// <summary>
    /// Its view model, kept beside the window because the window does not own it — the same split
    /// <see cref="DuplicatesWindow"/> makes, so a close can dispose it exactly once.
    /// </summary>
    private ChecksumViewModel? _checksumsVm;

    /// <summary>The modeless file-comparison window while one is open, re-pointed rather than stacked.</summary>
    private FileCompareWindow? _fileCompare;

    private FileCompareViewModel? _fileCompareVm;

    private TransferProgressWindow? _transferDetails;

    public MainWindow(
        ShellViewModel shell, BertBrowser.App.Services.AppSettings settings,
        BertBrowser.App.Services.Commands.KeymapService keymap)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        _keymap = keymap;
        DataContext = shell;

        // In the caption we draw ourselves, and in the taskbar and Alt-Tab with it. Set here rather
        // than in XAML because the version is only known at runtime.
        Title = $"BertBrowser {BertBrowser.App.Services.AppVersion.Display}".TrimEnd();

        _layoutHost = new PaneLayoutHost(shell, settings);
        PaneHostSite.Child = _layoutHost;

        // Throws if the catalogue and the handlers disagree, which is the point of building it here.
        Commands = BuildCommands();

        // Attached once, not per pane: the tree is shared, so N file-list controllers each hooking
        // its Drop would carry the same transfer out once per open pane.
        TreeDropTarget.Attach(FolderTree, shell);

        if (FolderTree.ContextMenu is { } treeMenu)
            treeMenu.Closed += (_, _) => ReleaseTreeShellMenuLater();

        ApplyWindowSettings();

        _shell.ActiveLocationChanged += OnActiveLocationChanged;
        _shell.TreeRevealRequested += OnTreeRevealRequested;
        _shell.PaneFocusRequested += OnPaneFocusRequested;
        _shell.GlobalSearchFocusRequested += FocusGlobalSearchBox;
        _shell.DiskUsageRequested += ShowDiskUsage;
        _shell.DuplicatesRequested += ShowDuplicates;
        _shell.ChangesRequested += ShowChanges;
        _shell.UndoHistoryRequested += ShowUndoHistory;
        _shell.ChecksumsRequested += ShowChecksums;
        _shell.ChecksumVerifyRequested += ShowChecksumVerify;
        _shell.FileCompareRequested += ShowFileCompare;
        _shell.SyncRequested += ShowSyncPreview;
        _shell.PropertyChanged += Shell_TransferProgressChanged;

        // Every layout pass rather than a list of size changes: the span the field sits in moves
        // with the window, the title, and either title-bar dropdown being shown.
        LayoutUpdated += (_, _) => CenterGlobalSearch();

        Loaded += async (_, _) => await _shell.InitializeAsync();
        Closing += (_, _) =>
        {
            // A change made in the last moment before closing would otherwise still be waiting on
            // the settings page's debounce.
            _settingsView?.ViewModel.Flush();
            SaveWindowSettings();
            // The undo history is session-only, so commit everything it is still holding — whatever
            // a Replace set aside, whatever a delete was holding on to, an archive's other version —
            // rather than leaving hidden staging folders behind.
            _shell.ReleaseUndoHistory();
        };
    }

    /// <summary>
    /// Puts the middle of the whole-PC search field on the middle of the title bar, as far as the
    /// span between the title and the buttons allows — in a narrow window it stops at whichever
    /// neighbour it would otherwise run under. The answer is worked out from the slot the group
    /// was handed, never from where the group is now, so setting the margin cannot feed back.
    /// </summary>
    private void CenterGlobalSearch()
    {
        if (TitleBarSearchGroup.Parent is not UIElement host || !TitleBarSearchGroup.IsDescendantOf(this))
            return;

        var slot = LayoutInformation.GetLayoutSlot(TitleBarSearchGroup);
        var slotLeft = host.TranslatePoint(slot.Location, this).X;
        var fieldLeft = GlobalSearchField.TranslatePoint(default, TitleBarSearchGroup).X;

        var wanted = (ActualWidth - GlobalSearchField.ActualWidth) / 2 - fieldLeft - slotLeft;
        var room = Math.Max(0, slot.Width - TitleBarSearchGroup.ActualWidth);
        var left = Math.Round(Math.Clamp(wanted, 0, room));

        if (Math.Abs(TitleBarSearchGroup.Margin.Left - left) < 0.5) return;
        TitleBarSearchGroup.Margin = new Thickness(left, 0, 0, 0);
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

        // The whole arrangement, so an elaborate split survives the session that built it.
        // LastPath stays as the fallback for a first launch and for a layout too damaged to reopen.
        _settings.Session = _shell.CaptureLayout();
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

    private void Settings_Click(object sender, RoutedEventArgs e) => ShowSettings((SettingsCategory?)null);

    /// <summary>The settings page while it is up, over <see cref="BrowserRoot"/>.</summary>
    private SettingsView? _settingsView;

    /// <summary>While this is true the keymap silences every shortcut about the folders —
    /// Backspace, Ctrl+T, F5 and the rest would otherwise act on panes nobody can see. See
    /// <c>KeymapRules.Dispatch</c>.</summary>
    internal bool IsSettingsOpen => _settingsView is not null;

    /// <summary>The page's view model while it is up, for the UI harness.</summary>
    internal SettingsViewModel? OpenSettings => _settingsView?.ViewModel;

    /// <summary>What <see cref="Settings_Applied"/> last pushed through the shell, taken when the
    /// page opens, so each write only redoes the parts whose values actually moved.</summary>
    private AppliedSettings? _lastApplied;

    private sealed record AppliedSettings(
        string? TileAspect, string Columns, bool RecordFileChanges, int RetentionHours,
        BertBrowser.Core.Services.UndoHistory.UndoBudget UndoBudget, string CustomCommands)
    {
        public static AppliedSettings Of(BertBrowser.App.Services.AppSettings settings) => new(
            settings.TileAspectRatio,
            string.Join("|", (settings.FileListColumns ?? []).Select(c => $"{c.Id}:{c.Width}")),
            settings.RecordFileChanges,
            settings.FileChangeRetentionHours,
            settings.EffectiveUndoBudget(),
            string.Join("|", settings.CustomCommands.Select(c => c.Id)));
    }

    /// <summary>Opens settings in place of the folders; <paramref name="page"/> opens it on a page
    /// other than General, which is how the change timeline points at its switch. Pressed again
    /// while it is up, it only changes page — there is only ever one.</summary>
    private void ShowSettings(SettingsCategory? page)
    {
        if (_settingsView is { } open)
        {
            if (page is { } category) open.ViewModel.ShowCategory(category);
            open.FocusCategories();
            return;
        }

        var vm = CreateSettingsViewModel();
        if (page is { } first)
            vm.ShowCategory(first);
        ShowSettings(vm);
    }

    /// <summary>The one construction site for the settings view model.</summary>
    private SettingsViewModel CreateSettingsViewModel()
    {
        var vm = new SettingsViewModel(
            _settings,
            App.Services.GetRequiredService<IThemeService>(),
            App.Services.GetRequiredService<BertBrowser.App.Services.IShellNewCatalog>(),
            App.Services.GetRequiredService<BertBrowser.App.Services.IFolderHandlerService>(),
            App.Services.GetRequiredService<BertBrowser.Core.Data.ChangeLogRepository>(),
            App.Services.GetRequiredService<BertBrowser.App.Services.Indexing.IndexAutoStartService>(),
            App.Services.GetRequiredService<BertBrowser.Core.Services.Mft.IMftIndexService>(),
            App.Services.GetRequiredService<IShellMenuSource>(),
            _shell.SavedWorkspaces,
            _shell.SavedSearches,
            _keymap);
        vm.ReadIndexerState();
        return vm;
    }

    /// <summary>
    /// Puts <paramref name="vm"/>'s page up in place of the folders. Internal for the UI harness,
    /// which builds its own view model (one that cannot register a sign-in task).
    /// </summary>
    internal void ShowSettings(SettingsViewModel vm)
    {
        if (_settingsView is not null) return;

        var view = new SettingsView(vm);
        view.BackRequested += (_, _) => CloseSettings();
        view.CustomiseThemeRequested += (_, _) => CustomiseTheme();
        view.WorkspaceRenameRequested += (_, item) => RenameWorkspace(item);
        view.WorkspaceDeleteRequested += (_, item) => _ = _shell.RemoveWorkspaceAsync(item);
        view.SavedSearchEditRequested += (_, item) => EditSavedSearch(item);
        view.SavedSearchDeleteRequested += (_, item) => _ = _shell.RemoveSavedSearchAsync(item);
        vm.Applied += Settings_Applied;
        _lastApplied = AppliedSettings.Of(_settings);
        _settingsView = view;

        GlobalSearchGroup.IsEnabled = false;
        CompareButton.IsEnabled = false;
        // Switching would replace panes nobody can see, and the page already lists them all.
        WorkspaceSwitcher.IsEnabled = false;
        SavedSearchSwitcher.IsEnabled = false;

        SettingsHost.Content = view;
        SettingsLayer.Visibility = Visibility.Visible;
        view.FocusCategories();
    }

    /// <summary>A click on the dimmed folders around the page means "back to them", as Esc does.</summary>
    private void SettingsScrim_MouseDown(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        CloseSettings();
    }

    /// <summary>
    /// Back to the folders, unless something on the page could not be saved and the user would
    /// rather stay and finish it. Returns whether the page closed.
    /// </summary>
    internal bool CloseSettings(bool ask = true)
    {
        if (_settingsView is not { } view) return true;

        if (!view.ViewModel.TryLeave(out var problem) && ask &&
            !MessageDialog.Show(this, $"{problem}\n\nLeave anyway? That entry and the rest of its list won't be saved.",
                "Settings", MessageDialogKind.Warning, showCancel: true, confirmLabel: "Leave"))
        {
            return false;
        }

        view.ViewModel.Applied -= Settings_Applied;
        _settingsView = null;
        SettingsLayer.Visibility = Visibility.Collapsed;
        SettingsHost.Content = null;

        GlobalSearchGroup.IsEnabled = true;
        CompareButton.ClearValue(IsEnabledProperty);
        WorkspaceSwitcher.IsEnabled = true;
        SavedSearchSwitcher.IsEnabled = true;

        _layoutHost.ActivePaneView?.FocusActiveTabList();
        return true;
    }

    /// <summary>The editor is modeless so its changes can be judged against the file list, which
    /// means the settings page has to get out of the way first.</summary>
    private void CustomiseTheme()
    {
        if (_settingsView is not { } view) return;
        var appearance = view.ViewModel.Appearance;
        if (!CloseSettings()) return;

        new ThemeEditorWindow(appearance) { Owner = this }.Show();
    }

    /// <summary>
    /// Pushes what the settings page just wrote through the shell. Runs on every write — the page
    /// applies as it goes, on a debounce — so everything here has to be cheap to repeat.
    /// </summary>
    private void Settings_Applied(object? sender, EventArgs e)
    {
        // Its setter refreshes the list and re-filters bookmarks, and does nothing when the value
        // is unchanged. (Custom-command menus rebuild on every open, so they need no refresh.)
        _shell.ShowHiddenItems = _settings.ShowHiddenItems;
        _shell.OpenDrivesInNewPanel = _settings.DrivesOpenTarget == BertBrowser.App.Services.DrivesOpenTarget.NewPanel;
        _shell.WorkspacesPlacement = _settings.WorkspacesPlacement;
        _shell.SavedSearchesPlacement = _settings.SavedSearchesPlacement;

        // The rest re-lay out every tab or talk to the index helper, so each goes only when its
        // own value moved rather than on every keystroke typed elsewhere on the page.
        var applied = AppliedSettings.Of(_settings);

        if (applied.TileAspect != _lastApplied?.TileAspect)
            _shell.RefreshTileAspect();
        // Reaches every tab that has not arranged its own columns. Without this the Columns page
        // would appear to do nothing until a new tab was opened.
        if (applied.Columns != _lastApplied?.Columns)
            _shell.ApplyColumnDefaults();
        // Turning recording off wipes the log, so this must not run on changes that are not its own.
        if (applied.RecordFileChanges != _lastApplied?.RecordFileChanges ||
            applied.RetentionHours != _lastApplied?.RetentionHours)
            App.ApplyChangeLogPolicy(_settings);
        // Lowering a limit releases entries, which commits what they held — only on its own change.
        if (applied.UndoBudget != _lastApplied?.UndoBudget)
            _ = _shell.SetUndoBudgetAsync(applied.UndoBudget);

        // The user's own commands can be given shortcuts, so one arriving or leaving changes what
        // the keymap can bind — and nothing else on the page does.
        if (applied.CustomCommands != _lastApplied?.CustomCommands)
            _keymap.Refresh();

        _lastApplied = applied;
        // "Start the indexer when BertBrowser launches" only matters next launch, and the
        // sign-in task applied itself the moment its box was ticked. Neither needs re-applying
        // here — this comment is so nobody adds a line and wonders why it does nothing.
    }

    /// <summary>
    /// Opens the transfer's detail view, or brings the one already up to the front. Re-used rather
    /// than stacked, and it is handed the shell's own progress view model — the same object the
    /// status bar is bound to, so the window is a second view of one transfer rather than a second
    /// copy of its state.
    /// </summary>
    private void TransferDetails_Click(object sender, RoutedEventArgs e) => ShowTransferDetails();

    private void ShowTransferDetails()
    {
        if (_shell.TransferProgress is null) return;

        if (_transferDetails is { IsLoaded: true })
        {
            _transferDetails.Activate();
            return;
        }

        _transferDetails = TransferProgressWindow.Show(this, _shell.TransferQueue);
        _transferDetails.Closed += (_, _) => _transferDetails = null;
    }

    /// <summary>
    /// The transfer finished, so its detail view has nothing left to show. Closed rather than left
    /// standing on figures that have stopped moving, which would read as one still running.
    /// </summary>
    private void Shell_TransferProgressChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(ShellViewModel.TransferProgress)) return;
        if (_shell.TransferProgress is null) _transferDetails?.Close();
    }

    /// <summary>
    /// The one construction site for the disk-usage view, reached only through
    /// <see cref="ShellViewModel.OpenDiskUsage"/> — the toolbar, both context menus and
    /// Ctrl+Shift+D all arrive here.
    /// </summary>
    /// <remarks>
    /// Modeless and re-used: opening it again on a different folder points the window that is
    /// already up at the new root rather than stacking a second one. Analysing is something you do
    /// repeatedly while browsing, and a pile of windows is the wrong answer to that.
    /// </remarks>
    private void ShowDiskUsage(string? path)
    {
        if (_diskUsage is { IsLoaded: true })
        {
            _diskUsage.Load(path);
            _diskUsage.Activate();
            return;
        }

        var vm = new DiskUsageViewModel(
            App.Services.GetRequiredService<IDiskUsageService>(),
            App.Services.GetRequiredService<IMftIndexService>(),
            _settings.ShowHiddenItems);

        _diskUsage = new DiskUsageWindow(vm, RevealFromDiskUsage) { Owner = this };
        _diskUsage.Closed += (_, _) => _diskUsage = null;
        _diskUsage.Show();
        _diskUsage.Load(path);
    }

    /// <summary>
    /// Says why comparing cannot run, in a modal.
    /// </summary>
    /// <remarks>
    /// The status bar is the wrong place for it. This is the answer to a button someone just
    /// pressed at the top of the window, and the footer of a two-pane window is about as far from
    /// their eyes as the text could be put — it was written, and never read.
    /// </remarks>
    private void ExplainNoCompare(string reason) =>
        MessageDialog.Show(this, reason, "Compare folders", MessageDialogKind.Information);

    /// <summary>
    /// Shows what a sync would do, and runs it if the user agrees.
    /// </summary>
    /// <remarks>
    /// Modal, unlike the disk-usage and duplicates windows, and for the reason a delete
    /// confirmation is: it is a question with an answer rather than a view to leave open — and the
    /// two folders behind it are about to change, which would leave a modeless copy describing a
    /// state that no longer exists.
    /// </remarks>
    private void ShowSyncPreview(CompareSessionViewModel session)
    {
        if (session.Result is not { } compare) return;

        var view = new SyncPreviewViewModel(
            remove => _shell.PlanSync(compare, remove),
            compare.LeftPath, compare.RightPath,
            (preview, show) => _shell.ExecuteSyncAsync(preview, show));

        SyncPreviewDialog.Show(this, view);
    }

    /// <summary>
    /// The one construction site for the duplicates view, reached only through
    /// <see cref="ShellViewModel.OpenDuplicates"/> — the toolbar, both context menus and
    /// Ctrl+Shift+U all arrive here.
    /// </summary>
    /// <remarks>
    /// Modeless and re-used, like the disk-usage view. It deliberately does <em>not</em> start
    /// scanning: unlike that one, this reads real files, and a window that began churning through a
    /// disk the moment it appeared would be a nasty surprise. It opens pointed at the folder, with
    /// Scan waiting to be pressed.
    /// </remarks>
    private void ShowDuplicates(string? path)
    {
        if (_duplicates is { IsLoaded: true })
        {
            _duplicates.Load(path);
            _duplicates.Activate();
            return;
        }

        var vm = new DuplicatesViewModel(
            App.Services.GetRequiredService<IDuplicateFinder>(),
            App.Services.GetRequiredService<IMftIndexService>(),
            RemoveDuplicateCopies,
            _settings.ShowHiddenItems,
            _settings.DuplicateMinSizeBytes,
            _settings.DuplicateSkipSystemFolders);

        _duplicates = new DuplicatesWindow(vm, RevealFromDiskUsage) { Owner = this };
        _duplicates.Closed += (_, _) =>
        {
            // The knobs are remembered rather than reset: someone who scans at 100 MB once will
            // scan at 100 MB again, and re-picking it every time is the kind of small friction
            // that stops a feature being used.
            _settings.DuplicateMinSizeBytes = vm.MinSizeBytes;
            _settings.DuplicateSkipSystemFolders = vm.SkipSystemFolders;
            _settings.Save();

            // This window was handed the view model, so closing it is where the subscriptions to
            // the index service go — the window itself does not dispose what it did not make.
            vm.Dispose();
            _duplicates = null;
        };
        _duplicates.Show();
        _duplicates.Load(path);
    }

    /// <summary>
    /// Compares two files by content. A fresh view model each time rather than a re-pointed one: the
    /// window's whole state is those two paths, and the two comparisons share nothing worth keeping.
    /// </summary>
    private void ShowFileCompare(string leftPath, string rightPath)
    {
        if (_fileCompare is { IsLoaded: true })
        {
            _fileCompare.Close();
            _fileCompare = null;
        }

        var vm = new FileCompareViewModel(
            App.Services.GetRequiredService<BertBrowser.Core.Services.Compare.IFileContentComparer>(),
            leftPath, rightPath);

        _fileCompareVm = vm;
        _fileCompare = new FileCompareWindow(vm) { Owner = this };
        _fileCompare.Closed += (_, _) =>
        {
            vm.Dispose();
            _fileCompare = null;
            _fileCompareVm = null;
        };
        _fileCompare.Show();
    }

    private void ShowChecksums(IReadOnlyList<string> paths) => ShowChecksumWindow(w => w.Load(paths));

    private void ShowChecksumVerify(string checksumFilePath) =>
        ShowChecksumWindow(w => w.LoadVerify(checksumFilePath));

    /// <summary>
    /// Opens the checksum window, or re-points the one already open, and then points it at
    /// something — which is the only part the two entry points differ in.
    /// </summary>
    private void ShowChecksumWindow(Action<ChecksumWindow> load)
    {
        if (_checksums is { IsLoaded: true })
        {
            load(_checksums);
            _checksums.Activate();
            return;
        }

        var vm = new ChecksumViewModel(
            App.Services.GetRequiredService<BertBrowser.Core.Services.Checksums.IFileDigester>(),
            _settings.ResolvedChecksumAlgorithms);

        _checksumsVm = vm;
        _checksums = new ChecksumWindow(vm) { Owner = this };
        _checksums.Closed += (_, _) =>
        {
            // Remembered rather than reset, like the duplicate finder's knobs: someone who works in
            // MD5 because that is what their download pages publish should not re-tick it every time.
            _settings.ChecksumAlgorithms = string.Join(",", vm.SelectedAlgorithms);
            _settings.Save();

            vm.Dispose();
            _checksums = null;
            _checksumsVm = null;
        };
        _checksums.Show();
        load(_checksums);
    }

    /// <summary>
    /// Removes the copies the duplicates view has marked, through the same plan, confirmation and
    /// undo history as any other delete in this app.
    /// </summary>
    /// <remarks>
    /// <b>Nothing here calls File.Delete.</b> Going through the planner is what keeps the
    /// protected-location refusals, the Recycle Bin, Ctrl+Z and the tab fan-out in force — a
    /// duplicate finder with its own delete path would be a second thing to audit and the more
    /// dangerous of the two.
    /// </remarks>
    private async Task<IReadOnlyCollection<string>> RemoveDuplicateCopies(IReadOnlyList<string> paths)
    {
        var plan = _shell.PlanDelete(
            [.. paths.Select(p => new DeleteSource(p, IsDirectory: false))], DeleteMode.Recycle);

        if (!plan.HasWork)
        {
            if (plan.Problems is { Count: > 0 } problems)
                MessageDialog.Show(this, string.Join("\n\n", problems.Select(p => p.Message)),
                    "Delete", MessageDialogKind.Warning);
            return [];
        }

        var owner = _duplicates is { IsLoaded: true } window ? (Window)window : this;
        if (!DeleteDialog.Confirm(owner, plan, _shell.SurveyDelete)) return [];

        var outcome = await _shell.DeleteAsync(plan);
        if (outcome.Failed.Count > 0)
            MessageDialog.Show(owner, string.Join("\n\n", outcome.Failed.Select(f => f.Message)),
                "Delete", MessageDialogKind.Warning);

        // Only what really went: an item the executor refused is still there, and dropping its row
        // would tell the user it had gone.
        return [.. outcome.Deleted.Select(d => d.SourcePath)];
    }

    private void TreeDuplicates_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenDuplicates(node.FullPath);
    }

    /// <summary>
    /// The one construction site for the change timeline, reached only through
    /// <see cref="ShellViewModel.OpenChanges"/> — the toolbar, both context menus and Ctrl+Shift+H
    /// all arrive here. Modeless and re-used, like the disk-usage view.
    /// </summary>
    private void ShowChanges(string? path)
    {
        if (_changes is { IsLoaded: true })
        {
            _changes.Load(path);
            _changes.Activate();
            return;
        }

        var vm = new ChangeTimelineViewModel(
            App.Services.GetRequiredService<BertBrowser.Core.Data.ChangeLogRepository>(),
            App.Services.GetRequiredService<IMftIndexService>(),
            _settings);

        _changes = new ChangeTimelineWindow(vm, RevealFromDiskUsage, () =>
        {
            // The page opens inside this window now, behind the timeline that asked for it.
            Activate();
            ShowSettings(SettingsCategory.History);
        }) { Owner = this };
        _changes.Closed += (_, _) => _changes = null;
        _changes.Show();
        _changes.Load(path);
    }

    private void TreeChanges_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenChanges(node.FullPath);
    }

    /// <summary>Acting on what the analysis says: a folder opens in a new tab, a file opens its
    /// folder with the file highlighted — the same reading of "show me this" the command line and
    /// the bookmarks list already use.</summary>
    private void RevealFromDiskUsage(string path, bool isDirectory)
    {
        if (isDirectory)
            _shell.OpenInNewTab(path);
        else
            _ = _shell.ActiveTab.RevealFileAsync(path);
    }

    // --- Whole-PC search (header) ---

    /// <summary>Puts the caret in the header field and selects whatever query is already there, so
    /// opening it again immediately overtypes the last search. Deferred one dispatcher pass: the
    /// field only became visible with this same property change, and an invisible element can't
    /// take focus.</summary>
    private void FocusGlobalSearchBox() =>
        Dispatcher.BeginInvoke(DispatcherPriority.Input, () =>
        {
            GlobalSearchBox.Focus();
            GlobalSearchBox.SelectAll();
        });

    private void GlobalSearchBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape) return;

        // Two-stage, exactly as the per-folder box is: while a content search is still reading,
        // Escape stops it and keeps what it found rather than throwing those results away.
        if (_shell.ActiveTab.IsSearchRunning)
        {
            _shell.ActiveTab.StopSearchCommand.Execute(null);
            e.Handled = true;
            return;
        }

        // Escape ends the search and hands focus back to the list, which is what the results were
        // showing. The field itself stays where it is — it is part of the chrome now.
        _shell.ActiveTab.ClearSearchCommand.Execute(null);
        _layoutHost.ActivePaneView?.FocusActiveTabList();
        e.Handled = true;
    }

    /// <summary>The query language, on one page. Modal, unlike the analysis windows: it is read
    /// and dismissed rather than worked alongside, and nothing behind it moves while it is up.</summary>
    private void SearchSyntax_Click(object sender, RoutedEventArgs e) => SearchSyntaxDialog.Show(this);

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
            BringTreeItemIntoView(container);
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
            BringTreeItemIntoView(container); // not connected to the visual tree yet
        }
    }

    private static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject =>
        VisualTreeUtil.FindDescendant<T>(root);

    /// <summary>Wires the tree's bring-into-view filter once its template is applied.</summary>
    private void FolderTree_Loaded(object sender, RoutedEventArgs e)
    {
        // Below the ScrollViewer, whose class handler would otherwise act on the request first.
        if (FindDescendant<ScrollViewer>(FolderTree) is { } scroller &&
            FindDescendant<ItemsPresenter>(scroller) is { } items)
        {
            items.RequestBringIntoView -= FolderTreeItems_RequestBringIntoView;
            items.RequestBringIntoView += FolderTreeItems_RequestBringIntoView;
        }
        FolderTree.PreviewMouseDown -= FolderTree_PreviewInput;
        FolderTree.PreviewMouseDown += FolderTree_PreviewInput;
        FolderTree.PreviewKeyDown -= FolderTree_PreviewInput;
        FolderTree.PreviewKeyDown += FolderTree_PreviewInput;
    }

    private void CollapseFolderTree_Click(object sender, RoutedEventArgs e) => _shell.Tree.CollapseAll();

    // Whether the tree's last input was a key press. Arrowing through the tree must follow the
    // selection off-screen; a click must never scroll it (the way VS Code's explorer behaves).
    private bool _treeKeyboardNavigating;
    private bool _treeExplicitBringIntoView;

    private void FolderTree_PreviewInput(object sender, InputEventArgs e) =>
        _treeKeyboardNavigating = e is KeyEventArgs;

    /// <summary>Selecting or focusing a row asks the ScrollViewer to bring it into view — for an
    /// expanded folder, its whole subtree — which is what scrolled the tree when a row was clicked.
    /// Only keyboard navigation and this window's own deliberate reveals may do that.</summary>
    private void FolderTreeItems_RequestBringIntoView(object sender, RequestBringIntoViewEventArgs e)
    {
        if (!_treeKeyboardNavigating && !_treeExplicitBringIntoView)
            e.Handled = true;
    }

    private void BringTreeItemIntoView(FrameworkElement container)
    {
        _treeExplicitBringIntoView = true;
        try
        {
            container.BringIntoView();
        }
        finally
        {
            _treeExplicitBringIntoView = false;
        }
    }

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

    /// <summary>Expands the tree's ancestors of the current directory, selects it and scrolls it
    /// into view, as VS Code's explorer reveals the open file. The folder itself keeps whatever
    /// expansion it had. Best-effort UI sugar — never throws.</summary>
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

        // Containers for freshly expanded nodes only exist after a layout pass.
        if (chain.Count > 0)
            _ = Dispatcher.InvokeAsync(() => ScrollTreeChainIntoView(chain), DispatcherPriority.Loaded);
    }

    private void BookmarkRow_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not BookmarkItemViewModel item) return;

        // Middle-click opens a pane of its own to the right, as it does over a folder anywhere
        // else. A file bookmark takes the folder it is in, which is what the pane menu items do.
        if (e.ChangedButton == MouseButton.Middle)
        {
            OpenBookmarkInNewPane(sender, SplitOrientation.Vertical);
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
            PrepareTreeMenu(node);
        }
        else
        {
            e.Handled = true; // portable device, empty area, or unexpanded placeholder: no menu
        }
    }

    /// <summary>Other programs' part of the tree menu, for one opening; see the file list's
    /// <c>_shellMenu</c> for the lifetime rule.</summary>
    private ShellMenuSession? _treeShellMenu;

    private IShellMenuSource? _shellMenus;

    private IShellMenuSource ShellMenus =>
        _shellMenus ??= App.Services.GetRequiredService<IShellMenuSource>();

    /// <summary>The folder tree's menu, for the UI harness to photograph after
    /// <see cref="PrepareTreeMenu"/>.</summary>
    internal ContextMenu TreeMenuForHarness =>
        FolderTree.ContextMenu ?? throw new InvalidOperationException("The folder tree has no context menu.");

    /// <summary>What a right-click on a tree node decides before its menu shows. Separate from the
    /// event so the harness can build the menu without opening it.</summary>
    internal void PrepareTreeMenu(DirectoryNodeViewModel node)
    {
        TreeBookmarkMenuItem.Header =
            _shell.Bookmarks.IsBookmarked(node.FullPath) ? "Remove bookmark" : "Bookmark";
        NewItemMenu.Rebuild(TreeNewMenuItem, TreeNewFileTypesSeparator, _settings,
            template => _ = CreateInTreeFolderAsync(node.FullPath, NewItemKind.File, template));

        if (FolderTree.ContextMenu is not { } menu) return;

        BuiltInMenu.Apply(menu, BuiltInMenu.Hidden(_settings));

        // A folder in the tree is an item in its parent, which is what the handlers are told; a
        // drive root has no parent and stands in for itself.
        _treeShellMenu?.Dispose();
        _treeShellMenu = ShellMenus.Open(
            [new ShellMenuTarget(node.FullPath, true)],
            ShellMenuContext.Items,
            Path.GetDirectoryName(node.FullPath) ?? node.FullPath);
        ContextMenuComposer.Compose(menu, _settings, [(node.FullPath, true)], _shell.RunCustomCommand,
            _treeShellMenu, () => new WindowInteropHelper(this).Handle, _shell.SetStatus);
        BuiltInMenu.TidySeparators(menu);
    }

    private void ReleaseTreeShellMenuLater()
    {
        var session = _treeShellMenu;
        if (session is null) return;

        _ = Dispatcher.BeginInvoke(DispatcherPriority.Background, () =>
        {
            if (ReferenceEquals(_treeShellMenu, session)) _treeShellMenu = null;
            session.Dispose();
        });
    }

    private void TreeBookmark_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _ = _shell.ToggleBookmarksAsync([(node.FullPath, true)]);
    }

    private void TreeDelete_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node) _ = DeleteTreeFolderAsync(node.FullPath);
    }

    private void TreeNewFolder_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _ = CreateInTreeFolderAsync(node.FullPath, NewItemKind.Folder);
    }

    private void TreeNewEmptyFile_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _ = CreateInTreeFolderAsync(node.FullPath, NewItemKind.File);
    }

    /// <summary>Creates something in a folder picked out of the tree. Goes through exactly the same
    /// plan and executor as the file list's own New — the tree is just another way of naming the
    /// folder — and the shell's PendingSelection is what selects the result in whichever pane
    /// happens to be showing it.</summary>
    private async Task CreateInTreeFolderAsync(
        string path, NewItemKind kind, NewFileTemplate? template = null)
    {
        var suggestion = _shell.SuggestNewItemName(path, kind, template);

        if (NewItemDialog.Show(this, path, kind, template, suggestion, _shell.PlanNewItem)
            is not { } plan)
        {
            return;
        }

        var outcome = await _shell.CreateNewItemAsync(plan);

        if (outcome.Failed is { } failed)
            MessageDialog.Show(this, failed.Message, "New", MessageDialogKind.Warning);
    }

    /// <summary>Deletes a folder picked out of the tree. Goes through exactly the same plan,
    /// confirmation and undo history as the file list's own delete — the tree is just another way of
    /// naming the folder. There is no permanent variant here: a drive root is one careless click
    /// away in this list, so the reversible one is the only one offered.</summary>
    private async Task DeleteTreeFolderAsync(string path)
    {
        var plan = _shell.PlanDelete([new DeleteSource(path, IsDirectory: true)], DeleteMode.Recycle);
        if (!plan.HasWork)
        {
            if (plan.Problems is { Count: > 0 } problems)
                MessageDialog.Show(this, string.Join("\n\n", problems.Select(p => p.Message)),
                    "Delete", MessageDialogKind.Warning);
            return;
        }

        if (!DeleteDialog.Confirm(this, plan, _shell.SurveyDelete)) return;

        var outcome = await _shell.DeleteAsync(plan);
        if (outcome.Failed.Count > 0)
            MessageDialog.Show(this, string.Join("\n\n", outcome.Failed.Select(f => f.Message)),
                "Delete", MessageDialogKind.Warning);
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
            _shell.OpenPortableDevice(device.Device);
    }

    // The row the user clicked, pinned to the viewport position it had at the moment of the click:
    // whatever the click sets off (selection, navigation reveal, expand/collapse reflow) the row
    // itself must not move under the cursor. _treeAnchorViewportY is that row top's offset from the
    // tree viewport. The anchor stays live until the user scrolls the tree or navigates elsewhere,
    // so every layout pass the click triggers — including ones that land long after mouse-up —
    // re-pins to the same offset.
    private ISidebarNode? _treeAnchorNode;
    private TreeViewItem? _treeAnchorContainer;
    private double _treeAnchorViewportY;

    /// <summary>Middle-click opens a pane of its own to the right instead of navigating — or, for
    /// a drive/device root (Depth == 0), follows <see cref="ShellViewModel.MiddleClickDriveOrDevice"/>
    /// (new tab or new pane per DrivesOpenTarget, not always a pane). Wired to the row's <c>PreviewMouseDown</c> rather than
    /// <see cref="FolderTreeItem_PreviewMouseDown"/>'s <c>PreviewMouseLeftButtonDown</c>, which
    /// never fires for a middle press. Handling it here (tunnelling, before <c>TreeViewItem</c>
    /// gets it) also stops the row being selected, which would otherwise navigate the active tab
    /// as well.</summary>
    private void FolderTreeItem_PreviewMiddleClick(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Middle) return;
        if (sender is not FrameworkElement { DataContext: ISidebarNode node }) return;

        if (node.Depth == 0)
            _shell.MiddleClickDriveOrDevice(node);
        else if (node is DirectoryNodeViewModel { FullPath.Length: > 0 } target)
            _shell.OpenInNewPane(target.FullPath, SplitOrientation.Vertical);
        e.Handled = true;
    }

    private void FolderTreeItem_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        // The second press of a double-click lands on the row the first one anchored; re-anchoring
        // there is harmless, since nothing has moved it since.
        ClearTreeAnchor();
        if (sender is not FrameworkElement { DataContext: ISidebarNode node }) return;

        // Mouse-down is the last moment the tree is still in its pre-click layout: this preview
        // event tunnels ahead of the bubbling one where TreeViewItem selects the row (which focuses
        // it, scrolls it into view, and kicks off the navigation reveal). Measure here, then undo
        // whatever that scrolled once the input event has been fully processed.
        AnchorTreeRow(node, FindAncestorTreeViewItem(sender as DependencyObject));
        ScheduleTreeAnchorRestore();

        // A single click only selects (and so navigates); a double-click opens or closes the
        // folder, drives included. Handled here rather than left to TreeViewItem's own
        // double-click toggle so the row stays pinned under the cursor through the reflow — and
        // again when lazily-loaded children arrive. Marking it handled keeps TreeViewItem from
        // toggling it straight back; MouseDoubleClick (portable devices) still fires, since
        // Control listens for handled presses too.
        if (e.ClickCount == 2 && node is DirectoryNodeViewModel { Children.Count: > 0 } dir)
        {
            dir.IsExpanded = !dir.IsExpanded;
            if (dir.IsExpanded)
                _ = RestoreTreeAnchorAfterPopulateAsync(dir);
            e.Handled = true;
        }
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

    private void TreeCopyPath_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is not { } node) return;

        _shell.ActiveTab.StatusText = BertBrowser.App.Services.FileClipboard.TrySetText(
            BertBrowser.Core.Paths.PathText.Quote(node.FullPath))
            ? "Copied 1 path"
            : "Could not copy — the clipboard is in use by another program.";
    }

    private void TreeDiskUsage_Click(object sender, RoutedEventArgs e)
    {
        if (_treeContextNode is { } node)
            _shell.OpenDiskUsage(node.FullPath);
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
