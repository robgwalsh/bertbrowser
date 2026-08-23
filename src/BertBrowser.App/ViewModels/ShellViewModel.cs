using System.ComponentModel;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using BertBrowser.Core.Cli;
using BertBrowser.Core.Data;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// The application shell: everything shared by every open directory. The directories themselves
/// live in <see cref="DirectoryTabViewModel"/>s — this owns the one folder tree, the one bookmark
/// list, the browse settings, the transfer/undo slot, and knows which tab is active so the window
/// chrome can follow it.
/// </summary>
public sealed partial class ShellViewModel : ObservableObject, IPaneHost
{
    private readonly ISearchService _searchService;
    private readonly IProcessLauncher _launcher;
    private readonly IFileTransferService _fileTransfer;
    private readonly IMftIndexService _mftIndex;
    private readonly AppSettings _settings;
    private readonly TransferPlanner _transferPlanner;
    private readonly TransferExecutor _transferExecutor;
    private readonly RenamePlanner _renamePlanner;
    private readonly RenameExecutor _renameExecutor;
    private readonly NewItemPlanner _newItemPlanner;
    private readonly NewItemExecutor _newItemExecutor;
    private readonly DeletePlanner _deletePlanner;
    private readonly DeleteExecutor _deleteExecutor;
    private readonly DeleteSurveyor _deleteSurveyor;
    private readonly PaneFactory _factory;

    /// <summary>"Show hidden items" browse setting, toggled from the Settings dialog. Mirrors
    /// <see cref="AppSettings.ShowHiddenItems"/>; hidden files/folders — and now hidden
    /// bookmarks — appear only while it is on.</summary>
    [ObservableProperty]
    private bool _showHiddenItems;

    partial void OnShowHiddenItemsChanged(bool value)
    {
        _settings.ShowHiddenItems = value;
        _settings.Save();
        Bookmarks.SetShowHidden(value);
        Tree.SetShowHidden(value);
        _ = RefreshAllTabsAsync();
    }

    public FolderTreeViewModel Tree { get; }
    public BookmarksViewModel Bookmarks { get; }

    /// <summary>How the open panes divide the window. Rebuilt by the view whenever
    /// <see cref="LayoutChanged"/> fires — never on plain navigation.</summary>
    public ILayoutNode<PaneViewModel> Layout { get; private set; }

    /// <summary>Raised after a split or a close, i.e. only when the arrangement itself changed.</summary>
    public event Action? LayoutChanged;

    /// <summary>Asks the view to put real keyboard focus in a pane, after a split or an F6.</summary>
    public event Action<PaneViewModel>? PaneFocusRequested;

    /// <summary>The pane the window chrome (toolbar, status bar, folder tree) follows.</summary>
    [ObservableProperty]
    private PaneViewModel _activePane;

    /// <summary>The directory the window chrome follows: the visible tab of the active pane.</summary>
    public DirectoryTabViewModel ActiveTab => ActivePane.ActiveTab!;

    /// <summary>MFT indexing state for the status bar ("Indexing C:…"); empty when idle. Also
    /// carries the reason when there is no index — a declined prompt, or a helper that died.</summary>
    [ObservableProperty]
    private string _indexingStatus = "";

    /// <summary>True when <see cref="IndexingStatus"/> is a failure the user could act on, which
    /// turns the status-bar text into a button.</summary>
    [ObservableProperty]
    private bool _indexingCanRetry;

    // --- Whole-PC search (header) ---

    /// <summary>Whether the header's whole-PC search shows its text field or the square button it
    /// collapses to. Window state rather than tab state: it is one control in the title bar, while
    /// the query behind it belongs to whichever tab is active (<c>ActiveTab.GlobalSearchText</c>),
    /// since that is the list the hits land in.</summary>
    [ObservableProperty]
    private bool _isGlobalSearchExpanded;

    /// <summary>Asks the view to put the caret in the header search field and select what's there.
    /// Expanding is a view-model decision; moving focus is the one part it can't do itself.</summary>
    public event Action? GlobalSearchFocusRequested;

    [RelayCommand]
    private void ExpandGlobalSearch()
    {
        IsGlobalSearchExpanded = true;
        GlobalSearchFocusRequested?.Invoke();
    }

    /// <summary>Collapses back to the button — refused while a whole-PC search is live, because the
    /// field is then the only thing saying what the file list is showing.</summary>
    [RelayCommand]
    private void CollapseGlobalSearch()
    {
        if (ActivePane.ActiveTab is { GlobalSearchText.Length: > 0 }) return;
        IsGlobalSearchExpanded = false;
    }

    /// <summary>Keeps the field open when the tab (or pane) coming to the front has a whole-PC
    /// search of its own, so switching to it doesn't hide the query producing its listing.</summary>
    private void SyncGlobalSearchExpansion()
    {
        if (ActivePane.ActiveTab is { GlobalSearchText.Length: > 0 })
            IsGlobalSearchExpanded = true;
    }

    /// <summary>Raised when the active tab's folder changes (or a different tab or pane becomes
    /// active), so the window can reveal it in the folder tree. Only ever raised for the active
    /// tab, which is what stops several open directories fighting over the tree's selection and
    /// scroll position.</summary>
    public event Action<string>? ActiveLocationChanged;

    public ShellViewModel(
        IFileSystemService fileSystem,
        ISearchService searchService,
        IFileTransferService fileTransfer,
        IBookmarkService bookmarkService,
        IMftIndexService mftIndex,
        DirSizeRepository dirSizes,
        TransferPlanner transferPlanner,
        TransferExecutor transferExecutor,
        RenamePlanner renamePlanner,
        RenameExecutor renameExecutor,
        NewItemPlanner newItemPlanner,
        NewItemExecutor newItemExecutor,
        DeletePlanner deletePlanner,
        DeleteExecutor deleteExecutor,
        DeleteSurveyor deleteSurveyor,
        PaneFactory factory,
        AppSettings settings,
        IProcessLauncher launcher)
    {
        _launcher = launcher;
        _searchService = searchService;
        _fileTransfer = fileTransfer;
        _mftIndex = mftIndex;
        _transferPlanner = transferPlanner;
        _transferExecutor = transferExecutor;
        _renamePlanner = renamePlanner;
        _renameExecutor = renameExecutor;
        _newItemPlanner = newItemPlanner;
        _newItemExecutor = newItemExecutor;
        _deletePlanner = deletePlanner;
        _deleteExecutor = deleteExecutor;
        _deleteSurveyor = deleteSurveyor;
        _factory = factory;
        _settings = settings;
        _showHiddenItems = settings.ShowHiddenItems; // seed the field so the ctor doesn't refresh

        Tree = new FolderTreeViewModel(fileSystem, dirSizes);
        Bookmarks = new BookmarksViewModel(bookmarkService);

        _activePane = new PaneViewModel(_factory, this);
        _activePane.AddTab("");
        _activePane.IsActivePane = true;
        _activePane.PropertyChanged += OnActivePanePropertyChanged;
        Layout = new LayoutLeaf<PaneViewModel>(_activePane);

        Tree.DirectorySelected += path => _ = ActiveTab.NavigateToAsync(path);
        _searchService.IndexRefreshed += OnIndexRefreshed;
        _mftIndex.IndexRefreshed += OnMftIndexRefreshed;
        _mftIndex.StatusChanged += OnMftStatusChanged;
        IndexingStatus = _mftIndex.StatusText;
        IndexingCanRetry = _mftIndex.CanRetry;
    }

    /// <summary>Overrides the initial directory (e.g. from the command line).</summary>
    public string? StartPath { get; set; }

    public async Task InitializeAsync()
    {
        Bookmarks.SetShowHidden(ShowHiddenItems);
        await Bookmarks.LoadAsync();

        // Before the drives load: the setting decides what each node's expander probe reports.
        Tree.SetShowHidden(ShowHiddenItems);

        // Drives are enumerated off-thread; the roots must exist before the first reveal.
        await Tree.LoadDrivesAsync();

        var start = StartPath ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        await ActiveTab.NavigateToAsync(start);

        // Portable devices can be slow to enumerate; append them after the first view loads.
        await Tree.LoadDevicesAsync();
    }

    // --- Panes and layout (IPaneHost) ---

    partial void OnActivePaneChanged(PaneViewModel? oldValue, PaneViewModel newValue)
    {
        if (oldValue is not null)
        {
            oldValue.IsActivePane = false;
            oldValue.PropertyChanged -= OnActivePanePropertyChanged;
        }
        newValue.IsActivePane = true;
        newValue.PropertyChanged += OnActivePanePropertyChanged;
        OnPropertyChanged(nameof(ActiveTab));
        SyncGlobalSearchExpansion();
        if (newValue.ActiveTab is { } tab)
            ActiveLocationChanged?.Invoke(tab.CurrentPath);
    }

    private void OnActivePanePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(PaneViewModel.ActiveTab)) return;
        // Every window-chrome binding hangs off ActiveTab, so switching tabs has to look like the
        // shell's own property changed.
        OnPropertyChanged(nameof(ActiveTab));
        SyncGlobalSearchExpansion();
    }

    public void ActivatePane(PaneViewModel pane)
    {
        if (ReferenceEquals(pane, ActivePane)) return;
        ActivePane = pane;
    }

    public bool CanClosePane => LayoutTree.Leaves(Layout).Skip(1).Any();

    public void SplitPane(PaneViewModel pane, SplitOrientation orientation, string? path)
    {
        if (LayoutTree.FindLeaf(Layout, pane) is not { } leaf) return;

        var created = new PaneViewModel(_factory, this);
        created.AddTab(path ?? pane.ActiveTab?.CurrentPath ?? "");

        Layout = LayoutTree.Split(Layout, leaf, orientation, created, out _);
        LayoutChanged?.Invoke();
        ActivatePane(created);
        PaneFocusRequested?.Invoke(created);
    }

    public void ClosePane(PaneViewModel pane)
    {
        if (LayoutTree.FindLeaf(Layout, pane) is not { } leaf) return;
        // Refused for the last pane: the window always shows a directory.
        if (LayoutTree.Close(Layout, leaf) is not { } layout) return;

        Layout = layout;

        // Move off the pane before tearing it down: ActiveTab reads through ActivePane, and every
        // window-chrome binding reads through ActiveTab.
        var wasActive = ReferenceEquals(pane, ActivePane);
        if (wasActive)
            ActivePane = LayoutTree.Leaves(Layout).First().Value;

        pane.Dispose();
        LayoutChanged?.Invoke();

        // After the rebuild, so the pane taking over already has a view to focus.
        if (wasActive) PaneFocusRequested?.Invoke(ActivePane);
    }

    public void NotifyLocation(PaneViewModel pane, DirectoryTabViewModel tab)
    {
        if (!ReferenceEquals(pane, ActivePane) || !ReferenceEquals(tab, pane.ActiveTab)) return;
        ActiveLocationChanged?.Invoke(tab.CurrentPath);
    }

    [RelayCommand]
    private void FocusNextPane() => StepPane(1);

    [RelayCommand]
    private void FocusPreviousPane() => StepPane(-1);

    private void StepPane(int step)
    {
        if (LayoutTree.FindLeaf(Layout, ActivePane) is not { } leaf) return;
        var next = LayoutTree.NextLeaf(Layout, leaf, step).Value;
        if (ReferenceEquals(next, ActivePane)) return;
        ActivatePane(next);
        PaneFocusRequested?.Invoke(next);
    }

    /// <summary>Opens a folder in another tab of the active pane. Background by convention: the
    /// point of a new tab is usually to come back to it, not to leave where you are.</summary>
    public void OpenInNewTab(string path, bool activate = false)
    {
        if (path.Length == 0) return;
        ActivePane.AddTab(path, activate);
    }

    /// <summary>Opens a folder beside (or below) the active pane, in a pane of its own.</summary>
    public void OpenInNewPane(string path, SplitOrientation orientation)
    {
        if (path.Length == 0) return;
        SplitPane(ActivePane, orientation, path);
    }

    /// <summary>
    /// Carries out a request from the command line, or from a second copy of the app handing over
    /// its own. Each target is resolved against disk here rather than in the parser, which is pure.
    /// </summary>
    /// <remarks>
    /// A target naming a file rather than a folder opens the folder it is in and highlights it —
    /// the same thing Explorer's <c>/select</c> does, and the only useful reading of "open this
    /// file" for a file browser.
    /// </remarks>
    public async Task OpenRequestAsync(CommandLineRequest request)
    {
        foreach (var target in request.Targets)
        {
            var (directory, selection) = Resolve(target);
            if (directory is null) continue;

            DirectoryTabViewModel tab;
            switch (request.Mode)
            {
                case OpenIn.NewTab:
                    tab = ActivePane.AddTab(directory, activate: true);
                    tab.PendingSelection = selection;
                    break;

                case OpenIn.NewPane:
                    SplitPane(ActivePane, SplitOrientation.Vertical, directory);
                    tab = ActiveTab; // SplitPane activates the pane it just created
                    tab.PendingSelection = selection;
                    break;

                default:
                    tab = ActiveTab;
                    // Set before navigating, not after: the listing that arrives is the one the
                    // selection belongs to, and by the time an await here returns the view has
                    // already been told about it. When the tab is *already* showing this folder
                    // there is no reload at all, and setting it is the only signal there will be.
                    tab.PendingSelection = selection;
                    await tab.NavigateToAsync(directory);
                    break;
            }
        }
    }

    /// <summary>Turns one requested target into "which folder to show, and what to highlight in
    /// it". Null when it names nothing that exists.</summary>
    private static (string? Directory, string? Selection) Resolve(OpenTarget target)
    {
        if (Directory.Exists(target.Path))
        {
            // A folder asked for with /select is highlighted in its parent, like Explorer.
            if (!target.Select) return (target.Path, null);
            var above = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(target.Path));
            return above is null ? (target.Path, null) : (above, target.Path);
        }

        if (File.Exists(target.Path))
            return (Path.GetDirectoryName(target.Path), target.Path);

        return (null, null);
    }

    /// <summary>Asks the window to reveal <paramref name="directory"/> in the folder tree. Ignored
    /// for anything but the active tab, so a background load never moves the tree.</summary>
    public void RequestTreeReveal(DirectoryTabViewModel tab, string directory)
    {
        if (!ReferenceEquals(tab, ActiveTab) || directory.Length == 0) return;
        TreeRevealRequested?.Invoke(directory);
    }

    /// <summary>Reveal a folder in the tree without changing what is being browsed (the file list's
    /// single-item selection mirrors into the tree).</summary>
    public event Action<string>? TreeRevealRequested;

    /// <summary>Routes a shell-level message (transfer, clipboard, bookmarks) to the status bar,
    /// which shows whichever tab is in front.</summary>
    public void SetStatus(string message) => ActiveTab.StatusText = message;

    // --- Fan-out over every open directory ---

    /// <summary>Every pane, in the order they appear on screen.</summary>
    public IEnumerable<PaneViewModel> AllPanes => LayoutTree.Leaves(Layout).Select(l => l.Value);

    /// <summary>Every open tab, in every pane.</summary>
    public IEnumerable<DirectoryTabViewModel> AllTabs => AllPanes.SelectMany(p => p.Tabs);

    /// <summary>The visible tab of each pane — what the user can actually see right now.</summary>
    public IEnumerable<DirectoryTabViewModel> VisibleTabs =>
        AllPanes.Select(p => p.ActiveTab).OfType<DirectoryTabViewModel>();

    public async Task RefreshAllTabsAsync()
    {
        foreach (var tab in AllTabs.ToList())
            await tab.RefreshViewAsync();
    }

    /// <summary>Re-reads the tile aspect ratio into every open list after Settings commits one.
    /// It changes only how the tiles are laid out, so it fans out without reloading anything.</summary>
    public void RefreshTileAspect()
    {
        foreach (var tab in AllTabs.ToList())
            tab.FileList.RefreshTileAspect();
    }

    /// <summary>Reloads every tab currently showing one of <paramref name="directories"/> — the
    /// point being that a move from one open folder to another has to update both of them, not
    /// just the one the drag started in.</summary>
    public async Task RefreshTabsShowingAsync(
        IEnumerable<string> directories, bool includeDescendants = false)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var directory in directories)
        {
            if (string.IsNullOrWhiteSpace(directory)) continue;
            try
            {
                keys.Add(PathKey.Canonicalize(directory));
            }
            catch (ArgumentException)
            {
                // Not a usable path (a deleted root, a device path) — nothing can be showing it.
            }
        }
        if (keys.Count == 0) return;

        // Snapshotted: a refresh can await, and tabs may be opened or closed while it does.
        foreach (var tab in AllTabs.ToList())
        {
            if (tab.CurrentPath.Length == 0) continue;
            string key;
            try
            {
                key = PathKey.Canonicalize(tab.CurrentPath);
            }
            catch (ArgumentException)
            {
                continue;
            }

            if (keys.Contains(key) || (includeDescendants && keys.Any(k => PathKey.IsUnder(key, k))))
                await tab.RefreshViewAsync();
        }
    }

    // --- Bookmarks ---

    /// <summary>Opens a bookmark: navigate into a folder, or reveal a bookmarked file in its
    /// containing folder.</summary>
    public async Task OpenBookmarkAsync(BookmarkItemViewModel? bookmark)
    {
        if (bookmark is null) return;

        if (bookmark.IsDirectory)
        {
            if (!Directory.Exists(bookmark.FullPath))
            {
                SetStatus($"Folder not found: {bookmark.FullPath}");
                return;
            }
            await ActiveTab.NavigateToAsync(bookmark.FullPath);
            return;
        }

        await ActiveTab.RevealFileAsync(bookmark.FullPath);
    }

    public async Task RemoveBookmarkAsync(BookmarkItemViewModel? bookmark)
    {
        if (bookmark is null) return;
        await Bookmarks.RemoveAsync(bookmark.FullPath);
    }

    /// <summary>Adds or removes bookmarks for the given entries. When any are not yet
    /// bookmarked, bookmarks them all; otherwise removes them all.</summary>
    public async Task ToggleBookmarksAsync(IReadOnlyList<(string FullPath, bool IsDirectory)> entries)
    {
        if (entries.Count == 0) return;

        var anyMissing = entries.Any(e => !Bookmarks.IsBookmarked(e.FullPath));
        foreach (var (fullPath, isDirectory) in entries)
        {
            if (anyMissing)
                await Bookmarks.AddAsync(fullPath, isDirectory);
            else
                await Bookmarks.RemoveAsync(fullPath);
        }
        SetStatus(anyMissing
            ? $"Bookmarked {entries.Count} item(s)"
            : $"Removed {entries.Count} bookmark(s)");
    }

    // --- Background index callbacks ---

    /// <summary>Only the visible tabs act on this: folder sizes are cheap per tab but not per tab
    /// times per pane, and a hidden tab catches up the moment it is brought to the front.</summary>
    private void OnMftIndexRefreshed(string rootKey)
    {
        Application.Current?.Dispatcher.InvokeAsync(() =>
        {
            foreach (var tab in AllTabs.ToList())
                tab.OnMftIndexRefreshed();

            // The tree is shared, so it refreshes once here rather than per tab: this is when
            // the folder sizes beside its names first exist.
            _ = Tree.RefreshSizesAsync();
        });
    }

    private void OnMftStatusChanged()
    {
        Application.Current?.Dispatcher.InvokeAsync(() =>
        {
            IndexingStatus = _mftIndex.StatusText;
            IndexingCanRetry = _mftIndex.CanRetry;
        });
    }

    /// <summary>
    /// Asks for the search index again after it was declined or lost.
    /// </summary>
    /// <remarks>
    /// A command rather than anything automatic, and that is the whole point: every retry raises a
    /// UAC prompt, so it happens when someone clicks and at no other time.
    /// </remarks>
    [RelayCommand]
    private void RetryIndexing() => _mftIndex.Retry();

    private void OnIndexRefreshed(string rootKey)
    {
        Application.Current?.Dispatcher.InvokeAsync(() =>
        {
            foreach (var tab in AllTabs.ToList())
                tab.OnIndexRefreshed(rootKey);
        });
    }

    // --- Clipboard (copy / cut / paste) ---

    [RelayCommand]
    private void CopySelection(IList<FileItemViewModel>? items) => SetClipboard(items, cut: false);

    [RelayCommand]
    private void CutSelection(IList<FileItemViewModel>? items) => SetClipboard(items, cut: true);

    private void SetClipboard(IList<FileItemViewModel>? items, bool cut)
    {
        var paths = items?.Select(i => i.FullPath).ToList();
        if (paths is not { Count: > 0 }) return;

        try
        {
            FileClipboard.SetFiles(paths, cut);
        }
        catch (System.Runtime.InteropServices.ExternalException ex)
        {
            SetStatus($"Clipboard error: {ex.Message}");
            return;
        }
        SetStatus($"{paths.Count} item(s) {(cut ? "cut" : "copied")}");
    }

    [RelayCommand]
    private async Task PasteAsync()
    {
        var destination = ActiveTab.CurrentPath;
        if (destination.Length == 0) return;

        (IReadOnlyList<string> Paths, bool IsCut)? clip;
        try
        {
            clip = FileClipboard.GetFiles();
        }
        catch (System.Runtime.InteropServices.ExternalException ex)
        {
            SetStatus($"Clipboard error: {ex.Message}");
            return;
        }
        if (clip is null) return;
        var (paths, isCut) = clip.Value;

        SetStatus(isCut ? "Moving…" : "Copying…");

        var errors = new List<string>();
        var pasted = 0;

        await Task.Run(() =>
        {
            foreach (var source in paths)
            {
                try
                {
                    if (isCut)
                    {
                        var dest = _fileTransfer.MoveInto(source, destination);
                        if (!dest.Equals(source, StringComparison.OrdinalIgnoreCase))
                            pasted++;
                    }
                    else
                    {
                        _fileTransfer.CopyInto(source, destination);
                        pasted++;
                    }
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                    or InvalidOperationException or FileNotFoundException or DirectoryNotFoundException)
                {
                    errors.Add(ex.Message);
                }
            }
        });

        if (isCut && pasted > 0)
        {
            try
            {
                FileClipboard.Clear(); // a cut is one-shot, like Explorer
            }
            catch (System.Runtime.InteropServices.ExternalException)
            {
            }
        }

        // A cut empties its source folders too, so every tab showing one of them is now wrong.
        var affected = paths.Select(Path.GetDirectoryName).OfType<string>().Append(destination);
        await RefreshTabsShowingAsync(affected);

        var verb = isCut ? "Moved" : "Copied";
        SetStatus(errors.Count > 0
            ? $"{verb} {pasted} item(s); {errors.Count} failed — {errors[0]}"
            : $"{verb} {pasted} item(s)");
    }

    // --- Rename ---

    /// <summary>Works out what renaming these to this pattern would produce, without changing
    /// anything — the rename dialog asks on every keystroke so it can preview and refuse.</summary>
    public RenamePlan PlanRename(IReadOnlyList<RenameSource> sources, string pattern) =>
        _renamePlanner.Plan(sources, pattern);

    /// <summary>Carries out a rename the dialog already planned, then refreshes the tree and every
    /// tab showing an affected folder — a selection made in a search result can span several. The
    /// outcome comes back so the view can report whatever failed.</summary>
    /// <remarks>The plan is advisory in the same way a drop plan is: it was built while the dialog
    /// was open, and the executor checks every name against live disk state before it writes.</remarks>
    public async Task<RenameOutcome> RenameAsync(RenamePlan plan)
    {
        // Shares the transfer flag: a rename and a drop both move things about, and Ctrl+Z must not
        // reach the previous operation's undo record while either is still running.
        if (IsTransferring || !plan.HasWork) return RenameOutcome.Empty;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus($"Renaming {plan.Work.Count:N0} item(s)…");
            var outcome = await Task.Run(() => _renameExecutor.Execute(plan));

            RetireUndoable();
            if (outcome.CanUndo)
            {
                _undoableRename = outcome;
                UndoDescription = $"Ctrl+Z: undo rename of {outcome.Completed.Count:N0} item(s)";
            }

            await RefreshAfterRenameAsync(outcome.Completed, plan.Renames);

            var status = $"Renamed {outcome.Completed.Count:N0} item(s)";
            if (outcome.Failed.Count > 0)
                status += $"; {outcome.Failed.Count:N0} failed — {outcome.Failed[0].Message}";
            else if (outcome.CanUndo) status += " — Ctrl+Z to undo";
            SetStatus(status);
            return outcome;
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>Reverses the last rename, then refreshes exactly as the rename itself did.</summary>
    private async Task UndoRenameAsync(RenameOutcome outcome)
    {
        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus("Undoing…");
            var result = await Task.Run(() => _renameExecutor.Undo(outcome));

            // Spent either way: a partial undo must not be replayed.
            _undoableRename = null;
            UndoDescription = "";

            await RefreshAfterRenameAsync(result.Completed, RenameExecutor.UndoPlan(outcome).Renames);

            SetStatus(result.Failed.Count == 0
                ? $"Undone — {result.Completed.Count:N0} name(s) put back"
                : $"Put back {result.Completed.Count:N0} name(s); {result.Failed.Count:N0} could not be — {result.Failed[0].Message}");
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    // --- Creating ---

    /// <summary>The name the New dialog opens with: the type's default, stepped aside to "(2)" if
    /// that is already taken, so it never opens on a name it would refuse.</summary>
    public string SuggestNewItemName(
        string directory, NewItemKind kind, NewFileTemplate? template = null) =>
        kind == NewItemKind.Folder
            ? _newItemPlanner.SuggestName(directory, "New folder", NewItemKind.Folder)
            : _newItemPlanner.SuggestName(
                directory,
                template?.DefaultBaseName ?? "New file",
                NewItemKind.File,
                template?.Extension ?? "");

    /// <summary>Works out what creating this would produce, without changing anything — the New
    /// dialog asks on every keystroke so it can refuse before anything is written.</summary>
    public NewItemPlan PlanNewItem(
        string directory, string name, NewItemKind kind, string? templatePath) =>
        _newItemPlanner.Plan(directory, name, kind, templatePath);

    public async Task<NewItemOutcome> CreateNewItemAsync(NewItemPlan plan)
    {
        // Shares the transfer flag: it is what "this app is writing" means, it keeps a create from
        // racing a paste, and the harness's quiescence check reads it.
        if (IsTransferring || !plan.HasWork) return NewItemOutcome.Empty;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            var outcome = await Task.Run(() => _newItemExecutor.Execute(plan));

            // Deliberately no RetireUndoable and no undo record. Creating is additive, exactly as
            // copying is, so Ctrl+Z is left pointing at whatever move, rename or delete came
            // before rather than being spent on something the user can simply delete.

            if (outcome.CreatedPath is { } created)
            {
                // Set before the refresh is awaited, so the listing that arrives is the one the
                // selection belongs to — the same rule /select obeys. Done here rather than in the
                // view so the tree's New lands in whichever pane is showing that folder.
                foreach (var tab in TabsShowing(plan.Directory)) tab.PendingSelection = created;
            }

            await RefreshAfterCreateAsync(plan);

            SetStatus(outcome.Failed is { } failed
                ? failed.Message
                : $"Created '{plan.Name}'");
            return outcome;
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    private async Task RefreshAfterCreateAsync(NewItemPlan plan)
    {
        // The tree only shows folders, so a new file is no reason to rebuild it — and a rebuild
        // costs containers, which is most of what the folder-tree rules are about.
        if (plan.Kind == NewItemKind.Folder)
            await Tree.RefreshDirectoriesAsync([plan.Directory]);

        await RefreshTabsShowingAsync([plan.Directory]);
    }

    /// <summary>Every open tab showing <paramref name="directory"/>, matched by path key rather
    /// than by string comparison.</summary>
    private IEnumerable<DirectoryTabViewModel> TabsShowing(string directory)
    {
        string wanted;
        try
        {
            wanted = PathKey.Canonicalize(directory);
        }
        catch (ArgumentException)
        {
            yield break;
        }

        foreach (var tab in AllTabs.ToList())
        {
            if (tab.CurrentPath.Length == 0) continue;
            string key;
            try
            {
                key = PathKey.Canonicalize(tab.CurrentPath);
            }
            catch (ArgumentException)
            {
                continue;
            }

            if (key == wanted) yield return tab;
        }
    }

    private async Task RefreshAfterRenameAsync(
        IReadOnlyList<CompletedRename> completed, IReadOnlyList<PlannedRename> attempted)
    {
        var directories = attempted
            .Select(r => Path.GetDirectoryName(r.SourcePath))
            .OfType<string>()
            .ToList();
        await Tree.RefreshDirectoriesAsync(directories);
        await RefreshTabsShowingAsync(directories);
        await FollowRenamedFoldersAsync(completed);
    }

    /// <summary>Moves any tab that was sitting inside a renamed folder over to its new path. Without
    /// this a pane browsing the folder — or something below it — would be left pointing at a name
    /// that no longer exists, which a refresh can only turn into an error.</summary>
    private async Task FollowRenamedFoldersAsync(IReadOnlyList<CompletedRename> completed)
    {
        foreach (var rename in completed)
        {
            if (!rename.IsDirectory) continue;

            var from = PathKey.NormalizeDisplay(rename.SourcePath);
            var fromKey = PathKey.Canonicalize(from);

            foreach (var tab in AllTabs.ToList())
            {
                if (tab.CurrentPath.Length == 0) continue;
                string key;
                string current;
                try
                {
                    current = PathKey.NormalizeDisplay(tab.CurrentPath);
                    key = PathKey.Canonicalize(current);
                }
                catch (ArgumentException)
                {
                    continue;
                }

                // Canonicalizing only changes case, so the old path's length indexes into the
                // tab's own path exactly as it does into the key.
                if (key == fromKey)
                    await tab.NavigateToAsync(rename.FinalPath);
                else if (PathKey.IsUnder(key, fromKey))
                    await tab.NavigateToAsync(
                        Path.Combine(rename.FinalPath, current[from.Length..].TrimStart('\\')));
            }
        }
    }

    // --- Delete ---

    /// <summary>Works out what deleting these would take with it, without changing anything — the
    /// confirmation dialog shows exactly this plan, and the executor is handed the same one.</summary>
    public DeletePlan PlanDelete(IReadOnlyList<DeleteSource> sources, DeleteMode mode) =>
        _deletePlanner.Plan(sources, mode);

    /// <summary>Measures a plan for the confirmation dialog. Off the UI thread and cancellable: a
    /// folder holding a hundred thousand files takes a moment to add up, and closing the dialog
    /// must not wait for it.</summary>
    public DeleteSurvey SurveyDelete(
        DeletePlan plan, CancellationToken ct, IProgress<DeleteMeasurement>? progress) =>
        _deleteSurveyor.Survey(plan, ct, progress);

    /// <summary>Carries out a delete the dialog already confirmed, then refreshes the tree and
    /// every tab showing an affected folder. The outcome comes back so the view can report whatever
    /// failed.</summary>
    /// <remarks>The plan is advisory in the same way a drop plan is: it was built before the
    /// confirmation was answered, and the executor re-checks every item against live disk state.</remarks>
    /// <summary>
    /// Removes items that a foreign application took with a move. Called only when
    /// <see cref="DragOutContract"/> says the target copied them and left the originals to us.
    /// </summary>
    /// <remarks>
    /// <para>
    /// This goes through the ordinary reversible delete rather than removing anything directly, and
    /// that is the whole point. An external window's say-so cannot reach past
    /// <c>DeletePlanner</c>'s refusals — a drive root, a protected location — the removal is a
    /// rename rather than a copy however large the tree is, and it claims the undo slot, so Ctrl+Z
    /// puts everything back if the drag went somewhere unexpected.
    /// </para>
    /// <para>
    /// Sources that have already gone are dropped rather than reported: that is what an optimized
    /// move looks like from here, and it is a success, not a failure.
    /// </para>
    /// </remarks>
    public async Task RemoveDraggedOutSourcesAsync(IReadOnlyList<string> paths)
    {
        var sources = paths
            .Where(p => File.Exists(p) || Directory.Exists(p))
            .Select(p => new DeleteSource(p, Directory.Exists(p)))
            .ToList();

        if (sources.Count == 0)
        {
            await RefreshTabsShowingAsync(ParentDirectoriesOf(paths));
            return;
        }

        var plan = PlanDelete(sources, DeleteMode.Recycle);
        if (!plan.HasWork)
        {
            await RefreshTabsShowingAsync(ParentDirectoriesOf(paths));
            return;
        }

        var outcome = await DeleteAsync(
            plan, $"Ctrl+Z: undo moving {plan.Deletions.Count:N0} item(s) out of BertBrowser");

        // DeleteAsync leaves "Deleted N item(s)" behind, which after a drag into another window
        // reads as though the drag destroyed them. Say what actually happened instead.
        if (outcome.Failed.Count == 0 && outcome.Deleted.Count > 0)
            SetStatus($"Moved {outcome.Deleted.Count:N0} item(s) out — Ctrl+Z to undo");
    }

    private static IEnumerable<string> ParentDirectoriesOf(IEnumerable<string> paths) =>
        paths.Select(Path.GetDirectoryName).OfType<string>().Distinct(StringComparer.OrdinalIgnoreCase);

    /// <param name="undoDescription">What Ctrl+Z will say. Defaults to describing a delete; a drag
    /// out of the app reuses this whole path but is not, to the user, a delete.</param>
    public async Task<DeleteOutcome> DeleteAsync(DeletePlan plan, string? undoDescription = null)
    {
        // Shares the transfer flag with moves and renames: Ctrl+Z must not reach the previous
        // operation's undo record while any of them is still running.
        if (IsTransferring || !plan.HasWork) return DeleteOutcome.Empty(plan.Permanent);

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus($"Deleting {plan.Deletions.Count:N0} item(s)…");

            var progress = new Progress<DeleteProgress>(p =>
                SetStatus(p.CurrentName.Length > 0
                    ? $"Deleting {p.Done + 1:N0} of {p.Total:N0} — {p.CurrentName}"
                    : ActiveTab.StatusText));

            var outcome = await Task.Run(
                () => _deleteExecutor.Execute(plan, CancellationToken.None, progress));

            RetireUndoable();
            if (outcome.CanUndo)
            {
                _undoableDelete = outcome;
                UndoDescription = undoDescription
                    ?? $"Ctrl+Z: undo delete of {outcome.Deleted.Count:N0} item(s)";
            }

            // Vacate first: a tab still pointing inside a folder that has gone would otherwise be
            // reloaded onto a missing path and flash an error on its way out of it.
            await LeaveDeletedFoldersAsync(outcome.Deleted);
            await RefreshAfterDeleteAsync(plan);

            var status = $"Deleted {outcome.Deleted.Count:N0} item(s)";
            if (outcome.Failed.Count > 0)
                status += $"; {outcome.Failed.Count:N0} failed — {outcome.Failed[0].Message}";
            else if (outcome.CanUndo) status += " — Ctrl+Z to undo";
            SetStatus(status);
            return outcome;
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>Puts the last delete back, then refreshes exactly as the delete itself did.</summary>
    private async Task UndoDeleteAsync(DeleteOutcome outcome)
    {
        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus("Undoing…");
            var result = await Task.Run(() => _deleteExecutor.Undo(outcome));

            // Spent either way: a partial undo must not be replayed. Whatever could not be put back
            // is still held, and goes when the next operation retires this record.
            _undoableDelete = null;
            UndoDescription = "";

            var directories = outcome.Deleted
                .Select(d => Path.GetDirectoryName(d.SourcePath))
                .OfType<string>()
                .ToList();
            await Tree.RefreshDirectoriesAsync(directories);
            await RefreshTabsShowingAsync(directories);

            SetStatus(result.Failed.Count == 0
                ? $"Undone — {result.Restored:N0} item(s) put back"
                : $"Put back {result.Restored:N0} item(s); {result.Failed.Count:N0} could not be — {result.Failed[0].Message}");
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>Reloads the folders the items vanished from. Driven by the plan rather than the
    /// outcome, so a folder whose delete failed is refreshed too — the failure may well be that
    /// something already moved.</summary>
    private async Task RefreshAfterDeleteAsync(DeletePlan plan)
    {
        var directories = plan.Deletions.Select(d => d.ParentPath).Where(p => p.Length > 0).ToList();
        await Tree.RefreshDirectoriesAsync(directories);
        await RefreshTabsShowingAsync(directories);
    }

    /// <summary>Moves any tab that was sitting inside a deleted folder up to where that folder was.
    /// Without this a pane browsing it — or something below it — would be left pointing at a path
    /// that no longer exists, which a refresh can only turn into an error.</summary>
    private async Task LeaveDeletedFoldersAsync(IReadOnlyList<DeletedItem> deleted)
    {
        foreach (var item in deleted)
        {
            if (!item.IsDirectory) continue;

            var goneKey = PathKey.Canonicalize(item.SourcePath);
            var parent = Path.GetDirectoryName(PathKey.NormalizeDisplay(item.SourcePath));
            if (string.IsNullOrEmpty(parent)) continue;

            foreach (var tab in AllTabs.ToList())
            {
                if (tab.CurrentPath.Length == 0) continue;
                string key;
                try
                {
                    key = PathKey.Canonicalize(tab.CurrentPath);
                }
                catch (ArgumentException)
                {
                    continue;
                }

                if (key == goneKey || PathKey.IsUnder(key, goneKey))
                    await tab.NavigateToAsync(parent);
            }
        }
    }

    /// <summary>Runs a user-defined command once per selected item it applies to.</summary>
    public void RunCustomCommand(CustomCommandDefinition command, IReadOnlyList<(string FullPath, bool IsDirectory)> targets)
    {
        var matched = targets
            .Where(t => t.IsDirectory ? command.AppliesToDirectories : command.AppliesToFiles)
            .ToList();

        if (matched.Count == 0) return;

        // Resolved once, up front: a program that isn't there is one message, not one per selected
        // item. Resolving also decides *which* program runs here rather than leaving a bare name
        // for the shell to look up — see ExecutablePath.
        if (_launcher.Resolve(command.Command) is not { } program)
        {
            SetStatus($"'{command.Name}' failed: '{command.Command}' was not found.");
            return;
        }

        foreach (var (fullPath, isDirectory) in matched)
        {
            var message = _launcher.Launch(
                program,
                CommandTemplate.Expand(command.Arguments, fullPath),
                isDirectory ? fullPath : Path.GetDirectoryName(fullPath) ?? "",
                command.RunElevated);

            if (message is null) continue;

            SetStatus($"'{command.Name}' failed: {message}");
            return;
        }

        SetStatus($"Ran '{command.Name}' on {matched.Count} item(s)");
    }

    // --- Drag-and-drop transfers ---

    /// <summary>The one-level undo slot: whichever of a move, a rename or a delete happened last,
    /// and only one of the three is ever set. Retiring is not free for two of them — a transfer
    /// commits any entries a Replace displaced into staging, and a delete erases what it was
    /// holding — so it goes through <see cref="RetireUndoable"/> rather than being overwritten.
    /// A rename has nothing set aside: its staging names are gone by the time it finishes.</summary>
    private TransferOutcome? _undoableTransfer;

    private RenameOutcome? _undoableRename;

    private DeleteOutcome? _undoableDelete;

    /// <summary>True while a drop is being carried out; blocks a second one from overlapping it.</summary>
    [ObservableProperty]
    private bool _isTransferring;

    public bool CanUndo =>
        (_undoableTransfer?.CanUndo == true || _undoableRename?.CanUndo == true ||
            _undoableDelete?.CanUndo == true) && !IsTransferring;

    /// <summary>"Undo move of 3 items" for the menu/tooltip; empty when there is nothing to undo.</summary>
    [ObservableProperty]
    private string _undoDescription = "";

    /// <summary>Works out what a drop would do, without changing anything. Called while the drag
    /// hovers, so the view can allow or refuse the drop and explain why.</summary>
    public TransferPlan PlanDrop(IReadOnlyList<string> sources, string destination, TransferVerb verb) =>
        _transferPlanner.Plan(sources, destination, verb);

    /// <summary>Carries out a planned drop off the UI thread, then refreshes the tree nodes and
    /// every open tab on both sides of the transfer.</summary>
    public async Task ExecuteDropAsync(
        TransferPlan plan, IReadOnlyDictionary<string, ConflictResolution>? resolutions)
    {
        if (IsTransferring || !plan.HasWork) return;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            var verbing = plan.Verb == TransferVerb.Move ? "Moving" : "Copying";
            SetStatus($"{verbing} {plan.Transfers.Count:N0} item(s)…");

            var progress = new Progress<TransferProgress>(p =>
                SetStatus(p.CurrentName.Length > 0
                    ? $"{verbing} {p.Done + 1:N0} of {p.Total:N0} — {p.CurrentName}"
                    : ActiveTab.StatusText));

            var outcome = await Task.Run(
                () => _transferExecutor.Execute(plan, resolutions, CancellationToken.None, progress));

            RetireUndoable();
            if (outcome.CanUndo)
            {
                _undoableTransfer = outcome;
                UndoDescription = $"Ctrl+Z: undo move of {outcome.Completed.Count:N0} item(s)";
            }

            await RefreshAfterTransferAsync(plan, outcome);
            SetStatus(DescribeOutcome(plan, outcome));
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>Reverses the last move, rename or delete — whichever the undo slot holds.</summary>
    [RelayCommand(CanExecute = nameof(CanUndo))]
    private async Task UndoAsync()
    {
        // At most one of these is ever set: each operation retires the slot before claiming it.
        if (_undoableRename is { } rename) await UndoRenameAsync(rename);
        else if (_undoableDelete is { } deletion) await UndoDeleteAsync(deletion);
        else if (_undoableTransfer is { } transfer) await UndoTransferAsync(transfer);
    }

    private async Task UndoTransferAsync(TransferOutcome outcome)
    {
        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus("Undoing…");
            var result = await Task.Run(() => _transferExecutor.Undo(outcome));

            // The record is spent either way: a partial undo must not be replayed.
            _undoableTransfer = null;
            UndoDescription = "";

            var directories = outcome.Completed
                .SelectMany(c => new[] { Path.GetDirectoryName(c.SourcePath), Path.GetDirectoryName(c.FinalPath) })
                .Append(outcome.DestinationDirectory)
                .OfType<string>()
                .ToList();
            await Tree.RefreshDirectoriesAsync(directories);
            await RefreshTabsShowingAsync(directories);

            SetStatus(result.Failed.Count == 0
                ? $"Undone — {result.Restored:N0} item(s) put back"
                : $"Put back {result.Restored:N0} item(s); {result.Failed.Count:N0} could not be restored — {result.Failed[0].Message}");
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>
    /// Drops the pending undo record. This is the moment data actually goes: anything a Replace
    /// displaced into staging, and everything a delete has been holding, is erased here and not
    /// before — up until now Ctrl+Z could have brought it back.
    /// </summary>
    public void RetireUndoable()
    {
        if (_undoableTransfer is { } outcome)
            TransferExecutor.CommitStaging(outcome);
        if (_undoableDelete is { } deletion)
            DeleteExecutor.CommitStaging(deletion);
        _undoableTransfer = null;
        _undoableRename = null;
        _undoableDelete = null;
        UndoDescription = "";
        UndoCommand.NotifyCanExecuteChanged();
    }

    private async Task RefreshAfterTransferAsync(TransferPlan plan, TransferOutcome outcome)
    {
        var directories = outcome.Completed
            .Select(c => Path.GetDirectoryName(c.SourcePath))
            .Append(plan.DestinationDirectory)
            .OfType<string>()
            .ToList();
        await Tree.RefreshDirectoriesAsync(directories);
        await RefreshTabsShowingAsync(directories);
    }

    private static string DescribeOutcome(TransferPlan plan, TransferOutcome outcome)
    {
        var verb = plan.Verb == TransferVerb.Move ? "Moved" : "Copied";
        var text = $"{verb} {outcome.Completed.Count:N0} item(s)";
        if (outcome.Skipped.Count > 0) text += $", skipped {outcome.Skipped.Count:N0}";
        if (outcome.Failed.Count > 0) text += $", {outcome.Failed.Count:N0} failed — {outcome.Failed[0].Message}";
        else if (outcome.CanUndo) text += " — Ctrl+Z to undo";
        return text;
    }

    // --- Built-in "Open in…" launchers (files and directories) ---

    /// <summary>Opens a terminal rooted at the item's folder: the folder itself for a
    /// directory, or the containing folder for a file. Prefers Windows Terminal, falls
    /// back to PowerShell.</summary>
    public void OpenInTerminal(string fullPath, bool isDirectory)
    {
        var dir = isDirectory ? fullPath : Path.GetDirectoryName(fullPath);
        if (string.IsNullOrEmpty(dir)) return;

        // Resolved, not attempted — see OpenInVSCode: a shell launch cannot report "not installed",
        // so a null resolve is what the old catch block used to be.
        if (_launcher.Resolve("wt.exe") is { } terminal)
        {
            if (_launcher.Launch(terminal, $"-d \"{dir}\"") is { } wtMessage)
                SetStatus(wtMessage);
            return;
        }

        if (_launcher.Resolve("powershell.exe") is not { } powershell)
        {
            SetStatus("Cannot open terminal: neither Windows Terminal nor PowerShell was found.");
            return;
        }

        if (_launcher.Launch(powershell, workingDirectory: dir) is { } message)
            SetStatus(message);
    }

    /// <summary>Opens an MTP/PTP device in Explorer — its contents are a shell namespace rather
    /// than a path the in-app list can read.</summary>
    public void OpenPortableDevice(PortableDevice device)
    {
        if (PortableDevices.OpenInExplorer(device, _launcher) is { } message)
            SetStatus(message);
    }

    /// <summary>Opens the file or folder in VS Code. Uses the <c>code</c> launcher on PATH,
    /// then falls back to the standard user/system install locations of Code.exe.</summary>
    public void OpenInVSCode(string fullPath, bool isDirectory)
    {
        // Each candidate is resolved rather than attempted: a launch through the shell reports
        // nothing back, so "is VS Code installed?" has to be answered before anything is handed
        // over. This is what the old catch-and-try-the-next-one did.
        string?[] candidates =
        [
            _launcher.Resolve("code"),
            _launcher.Resolve(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Programs", "Microsoft VS Code", "Code.exe")),
            _launcher.Resolve(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                "Microsoft VS Code", "Code.exe")),
        ];

        if (candidates.FirstOrDefault(c => c is not null) is not { } exe)
        {
            SetStatus("VS Code not found. Install it, or add 'code' to your PATH.");
            return;
        }

        if (_launcher.Launch(exe, $"\"{fullPath}\"") is { } message)
            SetStatus(message);
    }
}
