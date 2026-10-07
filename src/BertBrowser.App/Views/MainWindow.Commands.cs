using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using BertBrowser.App.Services.Commands;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Commands;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Preview;
using BertBrowser.Core.Services.Transfer;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// What every command in <see cref="CommandCatalog"/> does, and the one place a key press becomes
/// a command. Each handler is a line into the shell, a pane, a tab or a dialog this window already
/// knew how to show; nothing new happens here except that it can now be reached by name.
/// </summary>
public partial class MainWindow
{
    private readonly KeymapService _keymap;

    /// <summary>The commands, by id. Internal for the UI harness, which runs them the way a key
    /// press and the palette do.</summary>
    internal CommandRegistry Commands { get; private set; } = null!;

    private DirectoryTabView? ActiveTabView => _layoutHost.ActivePaneView?.ActiveTabView;

    // --- Keys ---

    /// <summary>
    /// Every shortcut the window has. A key press is turned into a chord, <see cref="KeymapRules"/>
    /// decides whose it is given what has focus, and the command runs through the registry.
    /// </summary>
    /// <remarks>
    /// This sees a key <em>before</em> the focused control, which the window's <c>KeyBinding</c>s
    /// never did — so a chord that is the command's here is marked handled whether or not the
    /// command could run. Otherwise an unavailable Ctrl+Shift+N would carry on to the list and
    /// type-ahead would jump the selection to a file beginning with "n".
    /// </remarks>
    protected override void OnPreviewKeyDown(KeyEventArgs e)
    {
        if (!e.Handled && KeyChordInterop.FromEvent(e) is { } chord &&
            KeymapRules.Dispatch(_keymap.Current, chord, FocusNow()) is { } command)
        {
            // With the palette up only the shortcuts that work from anywhere are live, and running
            // one of those means leaving it — except its own, which closes it by itself.
            // A held key is still the command's — it must not fall through to the control — but
            // only a stepping command runs again for it. Otherwise holding the palette's chord
            // flickers it open and shut, and holding "copy to the other pane" queues the same
            // files once per repeat.
            if (!e.IsRepeat || KeymapRules.RepeatsWhenHeld(command))
            {
                if (IsPaletteOpen && command != PaletteCommand) ClosePalette();
                Commands.TryExecute(command);
            }

            e.Handled = true;
        }

        base.OnPreviewKeyDown(e);
    }

    private const string PaletteCommand = "app.palette";

    private void Palette_Click(object sender, System.Windows.RoutedEventArgs e) => ShowPalette();

    /// <summary>What has the keyboard, as the keymap's rules need to know it.</summary>
    internal FocusState FocusNow()
    {
        var focus = Keyboard.FocusedElement switch
        {
            TextBoxBase { IsReadOnly: true } => FocusKind.TextReadOnly,
            TextBoxBase or PasswordBox or ComboBox { IsEditable: true } => FocusKind.TextEditable,
            _ when ActiveTabView is { IsFileListFocused: true } => FocusKind.FileList,
            _ => FocusKind.Other,
        };

        return new FocusState(
            focus, SettingsOpen: IsSettingsOpen, PaletteOpen: IsPaletteOpen, ContentBlocked: IsContentBlocked,
            // The Keyboard page is waiting for a chord, and the next key press is its answer.
            Recording: _settingsView?.ViewModel.IsCapturingKey == true,
            InPopup: IsFocusInPopup());
    }

    /// <summary>
    /// Whether the keyboard is in a context menu, a dropdown or any other popup rather than in the
    /// window itself.
    /// </summary>
    /// <remarks>
    /// A popup is a window of its own, but its key events are routed up through the element it was
    /// opened from — which is how a menu's commands reach their target — so they pass through
    /// this window's <see cref="OnPreviewKeyDown"/> on the way down. Asked by presentation source
    /// rather than by type, so it covers every popup there is and any added later.
    /// </remarks>
    private bool IsFocusInPopup()
    {
        if (Keyboard.FocusedElement is not System.Windows.DependencyObject focused) return false;

        var theirs = System.Windows.PresentationSource.FromDependencyObject(focused);
        return theirs is not null && !ReferenceEquals(theirs, System.Windows.PresentationSource.FromVisual(this));
    }

    // --- Commands ---

    private CommandRegistry BuildCommands()
    {
        var handlers = new Dictionary<string, CommandHandler>(StringComparer.Ordinal);

        void Add(string id, Action run, Func<string?>? unavailable = null, bool window = false) =>
            handlers.Add(id, new CommandHandler(run, unavailable, window));

        // A command the view models already expose: runnable exactly when its CanExecute says so.
        void Bound(string id, Func<ICommand?> command, string whyNot, object? parameter = null, bool window = false) =>
            Add(id,
                () => command()?.Execute(parameter),
                () => command() is { } c && c.CanExecute(parameter) ? null : whyNot,
                window);

        // A file verb: the active tab's, under FileVerbRules — the rule the right-click menu shows.
        void Verb(string id, FileVerb verb, bool window = false) =>
            Add(id, () => ActiveTabView?.RunVerb(verb), () => WhyNot(verb), window);

        var tab = () => _shell.ActiveTab;
        var pane = () => _shell.ActivePane;

        // --- Go ---
        Bound("nav.back", () => tab().BackCommand, "There is nowhere to go back to.");
        Bound("nav.forward", () => tab().ForwardCommand, "There is nowhere to go forward to.");
        Bound("nav.up", () => tab().UpCommand, "This is already the top.");
        Bound("nav.refresh", () => tab().RefreshCommand, "Nothing to refresh.");
        Add("nav.refresh-all", () => _ = _shell.RefreshAllTabsAsync());
        Add("nav.address-bar", () => ActiveTabView?.EditAddress());
        // The palette, opened on this folder with a trailing slash: the folders under it are
        // listed at once, and typing carries on from here rather than from an empty box.
        Add("nav.goto", () => ShowPalette(GoToSeed()));
        Add("nav.drive-root", () => GoTo(Path.GetPathRoot(tab().CurrentPath)),
            () => Path.GetPathRoot(tab().CurrentPath) is { Length: > 0 } root &&
                  !root.Equals(tab().CurrentPath, StringComparison.OrdinalIgnoreCase)
                ? null
                : "This is already the top of the drive.");
        Add("nav.home", () => GoTo(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile)));
        Add("nav.desktop", () => GoTo(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory)));
        Add("nav.documents", () => GoTo(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments)));
        Add("nav.downloads", () => GoTo(Interop.KnownFolders.Downloads));
        Add("nav.containing-folder", OpenContainingFolder,
            () => !tab().FileList.IsFlattened ? "Only in search results or a flat view — elsewhere the folder is the one showing."
                : InArchive ? "Not available inside an archive."
                : tab().SelectedItems.Count == 1 ? null
                : "Select one item.");

        // --- Tabs ---
        Bound("tab.new", () => pane().NewTabCommand, "");
        Bound("tab.duplicate", () => pane().DuplicateTabCommand, "");
        Bound("tab.close", () => pane().CloseTabCommand, "");
        Add("tab.close-others", () => pane().CloseOtherTabsCommand.Execute(null),
            () => pane().HasMultipleTabs ? null : "This is the only tab in the pane.");
        Add("tab.reopen", () => pane().ReopenClosedTab(),
            () => pane().CanReopenClosedTab ? null : "No tab has been closed in this pane.");
        Add("tab.next", () => pane().NextTabCommand.Execute(null),
            () => pane().HasMultipleTabs ? null : "This is the only tab in the pane.");
        Add("tab.previous", () => pane().PreviousTabCommand.Execute(null),
            () => pane().HasMultipleTabs ? null : "This is the only tab in the pane.");
        // The parameter is the slot as text, because that is what the command has always taken;
        // "8" means the last tab however many there are, and a slot past the end lands there too.
        for (var slot = 0; slot < 8; slot++)
        {
            var parameter = slot.ToString();
            Add($"tab.goto-{slot + 1}", () => pane().ActivateTabAtCommand.Execute(parameter));
        }
        Add("tab.goto-last", () => pane().ActivateTabAtCommand.Execute("8"));
        Add("tab.move-to-new-pane", () => pane().MoveTabToNewPaneCommand.Execute(null),
            () => pane().HasMultipleTabs ? null : "This is the pane's only tab — it already has a pane to itself.");
        Add("tab.close-right", CloseTabsToTheRight,
            () => ActiveTabIndex() < pane().Tabs.Count - 1 ? null : "There are no tabs to the right.");
        Add("tab.move-left", () => pane().MoveTab(ActiveTabIndex(), ActiveTabIndex() - 1),
            () => ActiveTabIndex() > 0 ? null : "This is already the first tab.");
        Add("tab.move-right", () => pane().MoveTab(ActiveTabIndex(), ActiveTabIndex() + 1),
            () => ActiveTabIndex() < pane().Tabs.Count - 1 ? null : "This is already the last tab.");
        Add("tab.move-to-other-pane", _shell.MoveActiveTabToOtherPane, () => NoOtherPane() ?? WhileComparing());

        // --- Panes ---
        Bound("pane.split-right", () => pane().SplitVerticalCommand, "");
        Bound("pane.split-below", () => pane().SplitHorizontalCommand, "");
        Add("pane.close", () => pane().ClosePaneCommand.Execute(null),
            () => _shell.CanClosePane ? null : "This is the only pane.");
        Add("pane.next", () => _shell.FocusNextPaneCommand.Execute(null),
            () => _shell.CanClosePane ? null : "This is the only pane.");
        Add("pane.previous", () => _shell.FocusPreviousPaneCommand.Execute(null),
            () => _shell.CanClosePane ? null : "This is the only pane.");
        Add("pane.swap", _shell.SwapPanes, () => NoOtherPane() ?? WhileComparing());
        Add("pane.equalise", _shell.EqualisePanes, NoOtherPane);
        // With one pane these two make the second, which is what somebody asking for "the other
        // pane" before there is one wants.
        Add("pane.open-here-in-other", () => _shell.OpenInOtherPane(tab().CurrentPath),
            () => tab().CurrentPath.Length > 0 ? null : "There is no folder here to open.");
        Add("pane.open-selected-in-other",
            () => { if (ActiveTabView?.SelectedSingleFolder() is { } folder) _shell.OpenInOtherPane(folder); },
            () => ActiveTabView?.SelectedSingleFolder() is not null ? null : "Select one folder.");
        Add("pane.copy-to-other", () => SendSelectionToOtherPane(TransferVerb.Copy),
            () => WhyNotSendToOtherPane(TransferVerb.Copy));
        Add("pane.move-to-other", () => SendSelectionToOtherPane(TransferVerb.Move),
            () => WhyNotSendToOtherPane(TransferVerb.Move));
        Add("pane.focus-tree", () => FolderTree.Focus());
        Add("pane.focus-list", () => _layoutHost.ActivePaneView?.FocusActiveTabList());

        // --- Selection ---
        var noRows = () => ActiveTabView is { RowCount: > 0 } ? null : "There is nothing here to select.";
        Add("select.all", () => ActiveTabView?.SelectAllRows(), noRows);
        Add("select.none", () => ActiveTabView?.SelectNoRows(),
            () => tab().SelectedItems.Count > 0 ? null : "Nothing is selected.");
        Add("select.invert", () => ActiveTabView?.InvertSelection(), noRows);
        Add("select.pattern", () => ActiveTabView?.SelectByPattern(select: true), noRows, window: true);
        Add("select.unpattern", () => ActiveTabView?.SelectByPattern(select: false),
            () => tab().SelectedItems.Count > 0 ? null : "Nothing is selected.", window: true);
        Add("select.files", () => ActiveTabView?.SelectWhere(row => !row.IsDirectory), noRows);
        Add("select.folders", () => ActiveTabView?.SelectWhere(row => row.IsDirectory), noRows);

        // --- File ---
        Verb("file.open", FileVerb.Open);
        Verb("file.run-as-admin", FileVerb.RunAsAdmin);
        Verb("file.open-new-tab", FileVerb.OpenInNewTab);
        Add("file.open-pane-right", () => ActiveTabView?.RunOpenInPane(SplitOrientation.Vertical),
            () => WhyNot(FileVerb.OpenInNewPane));
        Add("file.open-pane-below", () => ActiveTabView?.RunOpenInPane(SplitOrientation.Horizontal),
            () => WhyNot(FileVerb.OpenInNewPane));
        Verb("file.open-terminal", FileVerb.OpenInTerminal);
        Verb("file.open-vscode", FileVerb.OpenInVSCode);
        Add("file.new-folder", () => ActiveTabView?.RunNew(NewItemKind.Folder), () => WhyNot(FileVerb.New), window: true);
        Add("file.new-file", () => ActiveTabView?.RunNew(NewItemKind.File), () => WhyNot(FileVerb.New), window: true);
        Verb("file.rename", FileVerb.Rename, window: true);
        Verb("file.delete", FileVerb.Delete, window: true);
        // Inside an archive there is only one kind of delete, and Shift+Del has always done it:
        // an entry has no Recycle Bin, so there is no second, more destructive thing to mean. The
        // menu still leaves the permanent item out in there; the key keeps working.
        Add("file.delete-permanently",
            () => ActiveTabView?.RunVerb(InArchive ? FileVerb.Delete : FileVerb.DeletePermanently),
            () => WhyNot(InArchive ? FileVerb.Delete : FileVerb.DeletePermanently),
            window: true);
        Verb("file.properties", FileVerb.Properties, window: true);
        Verb("file.compress", FileVerb.Compress, window: true);
        Verb("file.extract-here", FileVerb.ExtractHere);
        Verb("file.extract-to", FileVerb.ExtractTo, window: true);
        Verb("file.unlock-archive", FileVerb.UnlockArchive, window: true);
        Verb("file.bookmark", FileVerb.Bookmark);
        Verb("file.checksum", FileVerb.Checksum, window: true);
        Verb("file.verify-checksums", FileVerb.VerifyChecksums, window: true);
        Verb("file.compare-files", FileVerb.CompareFiles, window: true);
        Verb("file.settle-by-content", FileVerb.SettleByContent);
        // The three below write beside or away from the selection, so they follow Copy's rule:
        // something selected, and never from inside an archive, where an entry has no path to
        // link to or copy from.
        Add("file.create-shortcut",
            () => { if (ActiveTabView is { } view) _ = _shell.CreateShortcutsInAsync(view.SelectedPathsInListOrder(), tab().CurrentPath); },
            () => WhyNot(FileVerb.Copy) ?? WhyNot(FileVerb.New));
        Add("file.copy-to", () => ActiveTabView?.TransferSelectionToChosenFolder(TransferVerb.Copy),
            () => WhyNot(FileVerb.Copy), window: true);
        Add("file.move-to", () => ActiveTabView?.TransferSelectionToChosenFolder(TransferVerb.Move),
            () => WhyNot(FileVerb.Cut), window: true);

        // --- Edit ---
        Verb("edit.cut", FileVerb.Cut);
        Verb("edit.copy", FileVerb.Copy);
        Verb("edit.paste", FileVerb.Paste);
        Verb("edit.copy-path", FileVerb.CopyPath);
        Verb("edit.copy-name", FileVerb.CopyName);
        Add("edit.copy-folder-path", CopyFolderPath,
            () => tab().CurrentPath.Length > 0 ? null : "There is no folder here.");
        Add("edit.paste-shortcut", PasteShortcut, () => WhyNot(FileVerb.Paste) ?? WhyNot(FileVerb.New));
        Bound("edit.undo", () => _shell.UndoCommand, "There is nothing to undo.");
        Bound("edit.redo", () => _shell.RedoCommand, "There is nothing to redo.");
        Add("edit.undo-history", ShowUndoHistory, window: true);
        Add("edit.clear-undo-history",
            () => UndoHistoryWindow.ConfirmAndClear(_shell, App.Services.GetRequiredService<IUserConfirm>()),
            () => _shell.History.Undoable.Count + _shell.History.Redoable.Count > 0
                ? null
                : "The undo history is already empty.",
            window: true);

        // --- View ---
        Bound("view.preview", () => tab().TogglePreviewCommand, "");
        Bound("view.metadata", () => tab().ToggleMetadataCommand, "");
        Bound("view.side-pane", () => tab().CycleSidePaneCommand, "");
        Bound("view.flat", () => tab().ToggleFlatCommand, "");
        // The same property the settings page's box sets; the shell saves it and refreshes every
        // tab. Browser context, so it cannot be pressed under that page and race its own write.
        Add("view.hidden", () => _shell.ShowHiddenItems = !_shell.ShowHiddenItems);
        Add("view.details", () => SetThumbnailScale(0),
            () => tab().FileList.IsThumbnailView ? null : "This is already the details view.");
        Add("view.thumbnails", () => SetThumbnailScale(_lastThumbnailScale),
            () => tab().FileList.IsThumbnailView ? "This is already the thumbnail view." : null);
        Add("view.toggle-thumbnails",
            () => SetThumbnailScale(tab().FileList.IsThumbnailView ? 0 : _lastThumbnailScale));
        Add("view.thumbs-larger", () => SetThumbnailScale(Math.Min(1, tab().FileList.ThumbnailScale + ThumbnailStep)),
            () => tab().FileList.ThumbnailScale < 1 ? null : "The thumbnails are as large as they go.");
        Add("view.thumbs-smaller", () => SetThumbnailScale(Math.Max(0, tab().FileList.ThumbnailScale - ThumbnailStep)),
            () => tab().FileList.IsThumbnailView ? null : "This is the details view — there are no thumbnails to shrink.");
        Add("view.sort-name", () => SortBy(ColumnCatalog.Name));
        Add("view.sort-size", () => SortBy(ColumnCatalog.Size));
        Add("view.sort-type", () => SortBy(ColumnCatalog.Type));
        Add("view.sort-modified", () => SortBy(ColumnCatalog.Modified));
        Add("view.sort-created", () => SortBy(ColumnCatalog.Created));
        Add("view.sort-extension", () => SortBy(ColumnCatalog.Extension));
        // SetSort reverses when asked for the column already in force, which is exactly this.
        Add("view.sort-reverse", () => tab().FileList.SetSort(tab().FileList.SortBy));
        Add("view.add-column", () => ActiveTabView?.ShowColumnPickerFromCommand(), window: true);
        Add("view.columns-default", () => ActiveTabView?.UseColumnsForNewTabs());
        Add("view.reset-columns", () => tab().FileList.ColumnLayout = null,
            () => tab().FileList.ColumnsCustomized ? null : "These are already the default columns.");

        // --- Search ---
        Add("search.folder", () => _layoutHost.ActivePaneView?.FocusSearchBox());
        Bound("search.pc", () => _shell.FocusGlobalSearchCommand, "");
        Add("search.clear", () => tab().ClearSearchCommand.Execute(null),
            () => tab().ActiveSearchText.Length > 0 ? null : "No search is showing.");
        Add("search.stop", () => tab().StopSearchCommand.Execute(null),
            () => tab().IsSearchRunning ? null : "No search is running.");
        // Only with something to save: the dialog would refuse an empty query anyway.
        Add("search.save", SaveActiveSearch,
            () => string.IsNullOrWhiteSpace(tab().ActiveSearchText) ? "There is no search to save." : null,
            window: true);
        Add("search.syntax", () => SearchSyntaxDialog.Show(this), window: true);

        // --- Tools ---
        Bound("tools.disk-usage", () => _shell.AnalyseDiskUsageCommand, "", window: true);
        Bound("tools.duplicates", () => _shell.FindDuplicatesCommand, "", window: true);
        Bound("tools.changes", () => _shell.ShowChangesCommand, "", window: true);
        Bound("tools.compare", () => _shell.CompareWithOtherPaneCommand, "");
        Bound("tools.compare-rescan", () => _shell.CompareSession?.RescanCommand,
            "Only while two panes are being compared.");
        Bound("tools.compare-sync", () => _shell.CompareSession?.SyncCommand,
            "Only once a comparison has found something to sync.", window: true);
        Add("tools.compare-differences",
            () => { if (_shell.CompareSession is { } session) session.DifferencesOnly = !session.DifferencesOnly; },
            () => _shell.CompareSession is null ? "Only while two panes are being compared." : null);
        Add("tools.indexer-dismiss", () => _shell.DismissIndexerBannerCommand.Execute(null),
            () => _shell.ShowIndexerBanner ? null : "The banner is not showing.");
        Bound("tools.transfer-cancel", () => _shell.TransferProgress?.CancelCommand, "Nothing is being transferred.");
        Add("tools.indexer-start", () => _shell.StartIndexerCommand.Execute(null),
            () => _shell.IndexerBannerCanStart ? null : "The search index is already running, or cannot be started here.");
        Add("tools.indexer-retry", () => _shell.RetryIndexingCommand.Execute(null),
            () => _shell.IndexingCanRetry ? null : "Indexing has not failed.");
        Bound("tools.transfer-pause", () => _shell.TransferQueue.PauseCommand, "Nothing is being transferred.");
        Bound("tools.transfer-resume", () => _shell.TransferQueue.ResumeCommand, "No transfer is paused.");
        Add("tools.transfer-details", ShowTransferDetails,
            () => _shell.TransferProgress is null ? "Nothing is being transferred." : null,
            window: true);

        // --- Preview ---
        var noPreview = () => tab().IsPreviewVisible ? null : "The preview pane is closed.";
        Add("preview.mode-auto", () => tab().Preview.SetModeCommand.Execute(PreviewMode.Auto), noPreview);
        Add("preview.mode-raw", () => tab().Preview.SetModeCommand.Execute(PreviewMode.Text), noPreview);
        Add("preview.mode-hex", () => tab().Preview.SetModeCommand.Execute(PreviewMode.Hex), noPreview);
        Add("preview.wrap", () => tab().Preview.ToggleWrapCommand.Execute(null), noPreview);
        Add("preview.fit", () => tab().Preview.ToggleFitCommand.Execute(null), noPreview);
        Add("preview.fit-width", () => ActiveTabView?.FitPreviewWidth(), noPreview);
        Add("preview.play-next", () => tab().Preview.PlayNextCommand.Execute(null), noPreview);
        Add("preview.auto-advance", () => tab().Preview.ToggleAutoAdvanceCommand.Execute(null), noPreview);
        Add("preview.loop", () => tab().Preview.ToggleLoopCommand.Execute(null), noPreview);
        Add("preview.autoplay", () => tab().Preview.ToggleAutoPlayCommand.Execute(null), noPreview);
        // Marked as needing a person: one plays media and the other opens a window of its own.
        var noMedia = () => noPreview() ?? (tab().Preview.HasMedia ? null : "Nothing playable is being previewed.");
        Add("preview.play", () => ActiveTabView?.TogglePreviewPlayback(), noMedia, window: true);
        Add("preview.fullscreen", () => ActiveTabView?.TogglePreviewFullScreen(), noMedia, window: true);

        // --- App ---
        Add(PaletteCommand, () => ShowPalette());
        Add("app.keyboard", () => ShowSettings(SettingsCategory.Keyboard));
        Add("app.settings", () => ShowSettings((SettingsCategory?)null));
        Add("app.customise-theme", OpenThemeEditor, window: true);
        Add("app.match-windows-theme", () =>
        {
            var theme = App.Services.GetRequiredService<IThemeService>();
            theme.SetFollowSystem(!theme.IsFollowingSystem);
        });
        Add("app.save-workspace", SaveWorkspace, window: true);
        Add("app.bookmark-folder",
            () => _ = _shell.ToggleBookmarksAsync([(tab().CurrentPath, true)]),
            () => tab().CurrentPath.Length == 0 ? "There is no folder here to bookmark."
                : InArchive ? "Not available inside an archive."
                : null);
        Add("app.exit", Close, window: true);

        return new CommandRegistry(handlers, CustomCommandHandler);
    }

    private bool InArchive => _shell.ActiveTab.FileList.IsInsideArchive;

    private int ActiveTabIndex() =>
        _shell.ActivePane.ActiveTab is { } active ? _shell.ActivePane.Tabs.IndexOf(active) : -1;

    private string? NoOtherPane() => _shell.OtherPane is null ? "There is no other pane — split this one first." : null;

    /// <summary>
    /// Why the panes may not change places during a comparison. Its left and right are fixed when it
    /// starts, and the banner, the tints and Sync all speak of "the right side": moving a pane
    /// would leave every one of them describing the other folder — and Sync deleting from it.
    /// </summary>
    private string? WhileComparing() =>
        _shell.CompareSession is null ? null : "Stop comparing first — the comparison is between these two panes as they stand.";

    /// <summary>What "Go to folder" starts the box with: where the tab is, ready to be typed on
    /// from. Nothing inside an archive, whose path is not somewhere a typed path can carry on.</summary>
    private string GoToSeed()
    {
        var path = _shell.ActiveTab.CurrentPath;
        if (path.Length == 0 || InArchive) return "";
        return path.EndsWith('\\') ? path : path + "\\";
    }

    private void GoTo(string? path)
    {
        if (path is { Length: > 0 }) _ = _shell.ActiveTab.NavigateToAsync(path);
    }

    private void CloseTabsToTheRight()
    {
        var pane = _shell.ActivePane;
        foreach (var later in pane.Tabs.Skip(ActiveTabIndex() + 1).ToList())
            pane.CloseTab(later);
    }

    /// <summary>
    /// Leaves a search result or a flat view for the folder the selected row is really in, with
    /// that row selected — "show me where this is".
    /// </summary>
    private async void OpenContainingFolder()
    {
        var tab = _shell.ActiveTab;
        if (tab.SelectedItems is not [var item]) return;

        // Both have to go, or the folder would arrive already filtered or already flattened and
        // the row would be shown among the same neighbours it had a moment ago.
        if (tab.ActiveSearchText.Length > 0) await tab.ClearSearchCommand.ExecuteAsync(null);
        await tab.SetFlatViewAsync(false);

        await tab.RevealFileAsync(item.FullPath);
    }

    private void CopyFolderPath()
    {
        var path = _shell.ActiveTab.CurrentPath;
        _shell.SetStatus(BertBrowser.App.Services.FileClipboard.TrySetText(path)
            ? "Copied this folder's path"
            : "Could not copy — the clipboard is in use by another program.");
    }

    /// <summary>The files on the clipboard as shortcuts here, rather than as copies of themselves.</summary>
    private void PasteShortcut()
    {
        (IReadOnlyList<string> Paths, bool IsCut)? clip;
        try
        {
            clip = BertBrowser.App.Services.FileClipboard.GetFiles();
        }
        catch (System.Runtime.InteropServices.ExternalException ex)
        {
            _shell.SetStatus($"Clipboard error: {ex.Message}");
            return;
        }

        if (clip is { } files)
            _ = _shell.CreateShortcutsInAsync(files.Paths, _shell.ActiveTab.CurrentPath);
    }

    // --- The other pane ---

    private void SendSelectionToOtherPane(TransferVerb verb)
    {
        if (ActiveTabView is not { } view || _shell.OtherPane?.ActiveTab is not { } target) return;
        _ = _shell.TransferToAsync(view.SelectedPathsInListOrder(), target.CurrentPath, verb);
    }

    /// <summary>The source follows Copy's or Cut's rule; the destination has to be somewhere a
    /// drop could land — a real folder, not a search result and not inside an archive, which is
    /// what the planner would refuse anyway, said before the key is pressed rather than after.</summary>
    private string? WhyNotSendToOtherPane(TransferVerb verb)
    {
        if (WhyNot(verb == TransferVerb.Move ? FileVerb.Cut : FileVerb.Copy) is { } source) return source;
        if (_shell.OtherPane?.ActiveTab is not { } target) return NoOtherPane();
        if (target.CurrentPath.Length == 0 || target.FileList.IsSearchResult)
            return "The other pane is not showing a folder.";
        return target.FileList.IsInsideArchive ? "The other pane is inside an archive." : null;
    }

    // --- View ---

    /// <summary>How far one press of "larger" or "smaller" moves the zoom slider.</summary>
    private const double ThumbnailStep = 0.15;

    /// <summary>What "Thumbnail view" goes back to: the last zoom this window was at, so toggling
    /// away and back does not lose the size somebody chose.</summary>
    private double _lastThumbnailScale = 0.3;

    private void SetThumbnailScale(double scale)
    {
        var list = _shell.ActiveTab.FileList;
        if (list.ThumbnailScale > 0) _lastThumbnailScale = list.ThumbnailScale;
        list.ThumbnailScale = scale;
        if (scale > 0) _lastThumbnailScale = scale;
    }

    /// <summary>Sorts ascending by a column, whatever was in force — unlike a header click, which
    /// reverses on a repeat. A command called "Sort by size" that sometimes sorted the other way
    /// round would be a command nobody could predict; "Reverse sort order" is the other half.</summary>
    private void SortBy(string column)
    {
        var list = _shell.ActiveTab.FileList;
        list.SetSort(column);
        if (list.SortDescending) list.SetSort(column);
    }

    /// <summary>Why the active tab cannot run a file verb right now, or null when it can.</summary>
    private string? WhyNot(FileVerb verb)
    {
        if (ActiveTabView is not { } view) return "No folder is open.";
        var state = view.VerbState(verb);
        return state.Enabled ? null : state.Reason;
    }

    /// <summary>The theme editor, from wherever it was asked for. With the settings page up it is
    /// that page's own appearance model, so what was being edited carries across.</summary>
    private void OpenThemeEditor()
    {
        if (_settingsView is not null)
        {
            CustomiseTheme();
            return;
        }

        var appearance = new AppearanceViewModel(App.Services.GetRequiredService<IThemeService>());
        new ThemeEditorWindow(appearance) { Owner = this }.Show();
    }
}
