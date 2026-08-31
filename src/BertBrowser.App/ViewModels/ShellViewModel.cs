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
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Elevation;
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
    private readonly DirSizeRepository _dirSizes;
    private readonly PaneFactory _factory;
    private readonly IElevatedOperationRunner _elevation;
    private readonly IElevationPrompt _elevationPrompt;

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
    /// <summary>Asks the view to put the caret in the header search field and select what's there.
    /// The one part of "focus the search box" a view model cannot do itself.</summary>
    public event Action? GlobalSearchFocusRequested;

    /// <summary>
    /// Ctrl+Shift+F, and the only thing the whole-PC field needs from the shell now.
    /// </summary>
    /// <remarks>
    /// The field used to collapse into a magnifier button, and three separate rules existed to
    /// decide when it could fold away: not while a search was live (the query is the only thing
    /// saying what the list is showing), not while the caret was in it, and forced open when a
    /// tab carrying a query came to the front. All of that was machinery in service of hiding a
    /// search box in a file browser. It stays open; the rules are gone with it.
    /// </remarks>
    [RelayCommand]
    private void FocusGlobalSearch() => GlobalSearchFocusRequested?.Invoke();

    /// <summary>Raised when the active tab's folder changes (or a different tab or pane becomes
    /// active), so the window can reveal it in the folder tree. Only ever raised for the active
    /// tab, which is what stops several open directories fighting over the tree's selection and
    /// scroll position.</summary>
    public event Action<string>? ActiveLocationChanged;

    /// <summary>
    /// Raised with the folder to analyse, or null for "This PC". The window builds the view — a
    /// view model has no business constructing one — but every entry point goes through
    /// <see cref="OpenDiskUsage"/> so there is one route and one place it can be opened wrongly.
    /// </summary>
    public event Action<string?>? DiskUsageRequested;

    /// <summary>Opens the disk-usage view on <paramref name="path"/>, or on the whole PC when it
    /// is null.</summary>
    public void OpenDiskUsage(string? path) => DiskUsageRequested?.Invoke(path);

    /// <summary>The Ctrl+Shift+D arm: analyse whatever the active tab is showing. A tab with no
    /// path yet (or one showing a search result) falls back to the whole PC rather than refusing.
    /// </summary>
    [RelayCommand]
    private void AnalyseDiskUsage() =>
        OpenDiskUsage(ActiveTab.CurrentPath is { Length: > 0 } path ? path : null);

    /// <summary>
    /// Raised with the folder to search for duplicates, or null for "This PC". Shaped exactly like
    /// <see cref="DiskUsageRequested"/>, and for the same reason: the window builds the view, and
    /// every entry point goes through <see cref="OpenDuplicates"/> so there is one route and one
    /// place it can be opened wrongly.
    /// </summary>
    public event Action<string?>? DuplicatesRequested;

    /// <summary>Opens the duplicates view on <paramref name="path"/>, or on the whole PC when it
    /// is null.</summary>
    public void OpenDuplicates(string? path) => DuplicatesRequested?.Invoke(path);

    /// <summary>The Ctrl+Shift+U arm: search whatever the active tab is showing. A tab with no path
    /// yet (or one showing a search result) falls back to the whole PC rather than refusing.
    /// </summary>
    [RelayCommand]
    private void FindDuplicates() =>
        OpenDuplicates(ActiveTab.CurrentPath is { Length: > 0 } path ? path : null);

    private readonly IArchiveBrowser _archives;
    private readonly IArchivePasswords _archivePasswords;
    private readonly ExtractPlanner _extractPlanner;
    private readonly ExtractExecutor _extractExecutor;
    private readonly ArchiveCreator _archiveCreator;
    private readonly ArchiveEditPlanner _archiveEditPlanner;
    private readonly ArchiveEditExecutor _archiveEditExecutor;

    public ShellViewModel(
        IFileSystemService fileSystem,
        ISearchService searchService,
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
        IProcessLauncher launcher,
        IArchiveBrowser archives,
        IArchivePasswords archivePasswords,
        ExtractPlanner extractPlanner,
        ExtractExecutor extractExecutor,
        ArchiveCreator archiveCreator,
        ArchiveEditPlanner archiveEditPlanner,
        ArchiveEditExecutor archiveEditExecutor,
        IElevatedOperationRunner elevation,
        IElevationPrompt elevationPrompt)
    {
        _elevation = elevation;
        _elevationPrompt = elevationPrompt;
        _archiveEditPlanner = archiveEditPlanner;
        _archiveEditExecutor = archiveEditExecutor;
        _archiveCreator = archiveCreator;
        _archives = archives;
        _archivePasswords = archivePasswords;
        _extractPlanner = extractPlanner;
        _extractExecutor = extractExecutor;
        _launcher = launcher;
        _searchService = searchService;
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
        _dirSizes = dirSizes;
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

    /// <summary>
    /// The arrangement to restore, or null to start with one pane on one directory.
    /// </summary>
    /// <remarks>
    /// Ignored when <see cref="StartPath"/> came from the command line: someone who typed a folder
    /// asked for that folder, and reopening six panes over the top of it would bury it.
    /// </remarks>
    public SessionLayout? StartLayout { get; set; }

    public async Task InitializeAsync()
    {
        Bookmarks.SetShowHidden(ShowHiddenItems);
        await Bookmarks.LoadAsync();

        // Before the drives load: the setting decides what each node's expander probe reports.
        Tree.SetShowHidden(ShowHiddenItems);

        // Drives are enumerated off-thread; the roots must exist before the first reveal.
        await Tree.LoadDrivesAsync();

        if (StartLayout is not { } layout || !await RestoreSessionAsync(layout))
        {
            var start = StartPath ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            await ActiveTab.NavigateToAsync(start);
        }

        // Portable devices can be slow to enumerate; append them after the first view loads.
        await Tree.LoadDevicesAsync();
    }

    /// <summary>
    /// Rebuilds the saved arrangement, or leaves everything alone and returns false.
    /// </summary>
    /// <remarks>
    /// Everything unusable has already been pruned away by <see cref="SessionLayoutRules"/>; what
    /// is left is turned into real panes through the same <see cref="PaneViewModel.AddTab"/> a
    /// split or a Ctrl+T goes through, so a restored pane is indistinguishable from one the user
    /// just made.
    /// </remarks>
    /// <summary>
    /// Rebuilds a saved arrangement and points every tab at its folder. Returns false, having
    /// changed nothing, when the layout is not one that can be restored.
    /// </summary>
    /// <remarks>
    /// Awaiting only the visible tab of each pane is the difference between opening a session and
    /// waiting for one: the tabs behind it navigate while the window is already usable.
    /// </remarks>
    public async Task<bool> RestoreSessionAsync(SessionLayout saved)
    {
        if (RestoreLayout(saved) is not { Count: > 0 } pending) return false;

        await Task.WhenAll(pending.Where(p => p.Visible).Select(p => p.Tab.NavigateToAsync(p.Path)));

        foreach (var background in pending.Where(p => !p.Visible))
            _ = background.Tab.NavigateToAsync(background.Path);

        return true;
    }

    /// <summary>A tab waiting to be pointed at its folder, and whether its pane is showing it.</summary>
    private readonly record struct PendingNavigation(
        DirectoryTabViewModel Tab, string Path, bool Visible);

    private List<PendingNavigation>? RestoreLayout(SessionLayout saved)
    {
        if (!SessionLayoutRules.IsUsable(saved)) return null;

        var pending = new List<PendingNavigation>();
        var built = BuildLayout(saved, pending, out var active);
        if (built is null || pending.Count == 0) return null;

        var previous = ActivePane;

        Layout = built;
        ActivePane = active ?? LayoutTree.Leaves(built).First().Value;

        // The pane the constructor made is not in the new tree, so it would otherwise leak its
        // tab's in-flight work and event subscriptions.
        previous.Dispose();

        LayoutChanged?.Invoke();
        return pending;
    }

    /// <remarks>
    /// Tabs are added with an empty path so <see cref="PaneViewModel.AddTab"/> starts no navigation
    /// of its own — every one is collected into <paramref name="pending"/> instead, so the caller
    /// can await the visible ones and let the rest run behind them.
    /// </remarks>
    private ILayoutNode<PaneViewModel>? BuildLayout(
        SessionLayout node, List<PendingNavigation> pending, out PaneViewModel? active)
    {
        active = null;

        if (!node.IsSplit)
        {
            if (node.Tabs is not { Count: > 0 } tabs) return null;

            var pane = new PaneViewModel(_factory, this);
            var visibleIndex = Math.Clamp(node.ActiveTabIndex, 0, tabs.Count - 1);

            for (var i = 0; i < tabs.Count; i++)
            {
                var created = pane.AddTab("", activate: false);
                if (tabs[i].SortBy is { Length: > 0 } sortBy)
                {
                    // Normalised rather than trusted: the file is hand-editable and may have been
                    // written by a newer build that knows columns this one does not.
                    created.FileList.SortBy = ColumnCatalog.SortSpec(sortBy).Id;
                    created.FileList.SortDescending = tabs[i].SortDescending;
                }
                created.FileList.RestoreColumns(tabs[i].Columns);
                pending.Add(new PendingNavigation(created, tabs[i].Path, i == visibleIndex));
            }

            pane.ActiveTab = pane.Tabs[visibleIndex];
            if (node.IsActivePane) active = pane;

            return new LayoutLeaf<PaneViewModel>(pane) { Weight = node.Weight };
        }

        var children = new List<ILayoutNode<PaneViewModel>>();
        foreach (var child in node.Children!)
        {
            if (BuildLayout(child, pending, out var childActive) is not { } built) continue;
            children.Add(built);
            active ??= childActive;
        }

        // LayoutTree forbids a split with fewer than two children, so anything that thin is hoisted
        // rather than handed over as a shape the live tree would reject.
        if (children.Count == 0) return null;
        if (children.Count == 1)
        {
            children[0].Weight = node.Weight;
            return children[0];
        }

        return new LayoutSplit<PaneViewModel>(
            node.Orientation ?? SplitOrientation.Vertical, children) { Weight = node.Weight };
    }

    /// <summary>
    /// The current arrangement, in the shape settings.json stores. Taken on the way out.
    /// </summary>
    public SessionLayout CaptureLayout() => CaptureNode(Layout);

    private SessionLayout CaptureNode(ILayoutNode<PaneViewModel> node)
    {
        if (node is LayoutSplit<PaneViewModel> split)
        {
            return new SessionLayout
            {
                Orientation = split.Orientation,
                Weight = split.Weight,
                Children = split.Children.Select(CaptureNode).ToList(),
            };
        }

        var pane = ((LayoutLeaf<PaneViewModel>)node).Value;
        var tabs = pane.Tabs.Where(t => t.CurrentPath.Length > 0).ToList();

        return new SessionLayout
        {
            Weight = node.Weight,
            Tabs = tabs.Select(t => new SessionTab
            {
                Path = t.CurrentPath,
                SortBy = t.FileList.SortBy,
                SortDescending = t.FileList.SortDescending,
                // Null unless this tab's columns were actually arranged, so an untouched tab keeps
                // following the saved default rather than freezing today's copy of it.
                Columns = t.FileList.ColumnsCustomized
                    ? t.FileList.ColumnLayout?.Select(c => c.Copy()).ToList()
                    : null,
            }).ToList(),
            ActiveTabIndex = pane.ActiveTab is { } visible ? Math.Max(0, tabs.IndexOf(visible)) : 0,
            IsActivePane = ReferenceEquals(pane, ActivePane),
        };
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
        if (newValue.ActiveTab is { } tab)
            ActiveLocationChanged?.Invoke(tab.CurrentPath);
    }

    private void OnActivePanePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(PaneViewModel.ActiveTab)) return;
        // Every window-chrome binding hangs off ActiveTab, so switching tabs has to look like the
        // shell's own property changed.
        OnPropertyChanged(nameof(ActiveTab));
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

        // A bare archive keeps reveal-and-select. "bertbrowser C:\x\a.zip" typed at a prompt most
        // plausibly means "show me that file", and the shell folder-handler registration only ever
        // sends directories, so nothing is lost by not entering it here.
        if (File.Exists(target.Path))
            return (Path.GetDirectoryName(target.Path), target.Path);

        // A path *inside* an archive has no other reading, and until now was dropped in silence.
        if (BertBrowser.Core.Services.Archives.ArchivePath.Parse(target.Path, File.Exists) is not null)
            return (target.Path, null);

        return (null, null);
    }

    /// <summary>Asks the window to reveal <paramref name="directory"/> in the folder tree. Ignored
    /// for anything but the active tab, so a background load never moves the tree.</summary>
    public void RequestTreeReveal(DirectoryTabViewModel tab, string directory)
    {
        if (!ReferenceEquals(tab, ActiveTab) || directory.Length == 0) return;

        // The tree shows folders on disk, so somewhere inside an archive is revealed as the folder
        // holding the container. That is also where Up goes from the archive's own root, so the
        // highlight agrees with the navigation rather than pointing at a row that cannot exist.
        // Without this the tree would settle on the deepest ancestor it could reach anyway; naming
        // the rule is what stops that being mistaken for a bug later.
        if (BertBrowser.Core.Services.Archives.ArchivePath.Parse(directory, File.Exists)
            is { } inArchive)
        {
            directory = Path.GetDirectoryName(inArchive.ArchiveFile) ?? directory;
            if (directory.Length == 0) return;
        }

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

    /// <summary>
    /// Pushes a newly saved default column set into every tab that has not arranged its own.
    /// </summary>
    /// <remarks>
    /// Shaped like <see cref="RefreshTileAspect"/> — a re-layout, not a reload, since the rows are
    /// unchanged and only the cells around them differ. Without it the Settings page would be a
    /// control that visibly does nothing until you open a new tab; with it, a tab someone has
    /// arranged by hand is still left alone, which is what <c>ColumnsCustomized</c> is for.
    /// </remarks>
    public void ApplyColumnDefaults()
    {
        foreach (var tab in AllTabs.ToList())
            tab.FileList.ApplyDefaultColumns(_settings.FileListColumns);
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

    /// <summary>
    /// Copies the selection as text rather than as files: quoted full paths, one per line.
    /// </summary>
    /// <remarks>
    /// A separate clipboard shape from Copy on purpose. This one is for pasting into a terminal, an
    /// editor or a message, and putting a file drop there instead would make the receiving app
    /// offer to copy the files.
    /// </remarks>
    [RelayCommand]
    private void CopyPaths(IList<FileItemViewModel>? items) =>
        SetClipboardText(items, PathText.ForClipboard, "path");

    [RelayCommand]
    private void CopyNames(IList<FileItemViewModel>? items) =>
        SetClipboardText(items, PathText.NamesForClipboard, "name");

    private void SetClipboardText(
        IList<FileItemViewModel>? items, Func<IEnumerable<string>, string> format, string noun)
    {
        var paths = items?.Select(i => i.FullPath).ToList();
        if (paths is not { Count: > 0 }) return;

        // The clipboard belongs to the whole session and another process can be holding it; a
        // failed copy is worth a status line rather than an exception.
        ActiveTab.StatusText = FileClipboard.TrySetText(format(paths))
            ? $"Copied {paths.Count} {noun}{(paths.Count == 1 ? "" : "s")}"
            : "Could not copy — the clipboard is in use by another program.";
    }

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

        // Paste is a drop by another name, so it goes through the one planner and executor that
        // relocate user data — which is what gives it byte progress, cancellation, undo and
        // conflict handling, and leaves exactly one implementation to audit.
        var plan = PlanDrop(paths, destination, isCut ? TransferVerb.Move : TransferVerb.Copy);
        if (!plan.HasWork)
        {
            SetStatus(plan.Problems.Count > 0
                ? $"Nothing pasted — {plan.Problems[0].Message}"
                : "Nothing to paste here");
            return;
        }

        var outcome = await ExecuteDropAsync(plan, resolutions: null);

        // A cut is one-shot, like Explorer — but only once something has actually moved. A paste
        // that was refused, skipped or cancelled leaves the clipboard alone, so it can be tried
        // again somewhere else.
        if (isCut && outcome is { Completed.Count: > 0 })
        {
            try
            {
                FileClipboard.Clear();
            }
            catch (System.Runtime.InteropServices.ExternalException)
            {
            }
        }
    }

    // --- Rename ---

    /// <summary>Works out what renaming these to this pattern would produce, without changing
    /// anything — the rename dialog asks on every keystroke so it can preview and refuse.</summary>
    public RenamePlan PlanRename(IReadOnlyList<RenameSource> sources, string pattern) =>
        _renamePlanner.Plan(sources, pattern);

    /// <summary>The same question for the dialog's expanded panel, where the name comes from a
    /// find/replace, a case transform, a counter and a date rather than from typed text.</summary>
    /// <remarks>Only the naming differs: this goes through the same planner, and everything it
    /// decides about collisions and refusals is shared with the plain path.</remarks>
    public RenamePlan PlanRename(IReadOnlyList<RenameSource> sources, RenameRule rule) =>
        _renamePlanner.Plan(sources, rule);

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
            var (outcome, elevated) = await ElevateIfRefusedAsync(
                plan, await Task.Run(() => _renameExecutor.Execute(plan)));

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
            SetStatus(status + elevated);
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
            // A rename is its own inverse, so the undo is an ordinary rename of the undo plan — and
            // the ordinary retry covers it, with no undo-specific verb on the wire at all.
            var undoPlan = RenameExecutor.UndoPlan(outcome);
            var (result, elevated) = await ElevateIfRefusedAsync(
                undoPlan, await Task.Run(() => _renameExecutor.Execute(undoPlan)));

            // Spent either way: a partial undo must not be replayed.
            _undoableRename = null;
            UndoDescription = "";

            await RefreshAfterRenameAsync(result.Completed, undoPlan.Renames);

            SetStatus((result.Failed.Count == 0
                ? $"Undone — {result.Completed.Count:N0} name(s) put back"
                : $"Put back {result.Completed.Count:N0} name(s); {result.Failed.Count:N0} could not be — {result.Failed[0].Message}") + elevated);
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
            var (outcome, elevated) = await ElevateIfRefusedAsync(
                plan, await Task.Run(() => _newItemExecutor.Execute(plan)));

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

            SetStatus((outcome.Failed is { } failed
                ? failed.Message
                : $"Created '{plan.Name}'") + elevated);
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

            var (outcome, elevated) = await ElevateIfRefusedAsync(
                plan,
                await Task.Run(() => _deleteExecutor.Execute(plan, CancellationToken.None, progress)));

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
            SetStatus(status + elevated);
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
            var (result, elevated) = await ElevateIfRefusedAsync(
                outcome, await Task.Run(() => _deleteExecutor.Undo(outcome)));

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

            SetStatus((result.Failed.Count == 0
                ? $"Undone — {result.Restored:N0} item(s) put back"
                : $"Put back {result.Restored:N0} item(s); {result.Failed.Count:N0} could not be — {result.Failed[0].Message}") + elevated);
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

    /// <summary>
    /// A fourth arm, and genuinely a fourth thing rather than one of the others in disguise: undoing
    /// it renames a whole container back, not an item. It carries staging like a transfer and a
    /// delete do, so it retires through <see cref="RetireUndoable"/> too.
    /// </summary>
    private ArchiveEditOutcome? _undoableArchiveEdit;

    /// <summary>True while a drop is being carried out; blocks a second one from overlapping it.</summary>
    [ObservableProperty]
    private bool _isTransferring;

    /// <summary>
    /// The live byte-level state of a running transfer, or null when none is running.
    /// </summary>
    /// <remarks>
    /// Nullable, and bound through <c>NullToCollapsed</c> the way <see cref="IndexingStatus"/> is,
    /// rather than gated on <see cref="IsTransferring"/>: the status-bar strip and the detail window
    /// then share one source of truth and can be posed for a capture without pretending a transfer
    /// is under way — which would hang the harness's own busy-wait.
    /// </remarks>
    [ObservableProperty]
    private TransferProgressViewModel? _transferProgress;

    public bool CanUndo =>
        (_undoableTransfer?.CanUndo == true || _undoableRename?.CanUndo == true ||
            _undoableDelete?.CanUndo == true || _undoableArchiveEdit?.CanUndo == true)
        && !IsTransferring;

    /// <summary>"Undo move of 3 items" for the menu/tooltip; empty when there is nothing to undo.</summary>
    [ObservableProperty]
    private string _undoDescription = "";

    /// <summary>Works out what a drop would do, without changing anything. Called while the drag
    /// hovers, so the view can allow or refuse the drop and explain why.</summary>
    public TransferPlan PlanDrop(IReadOnlyList<string> sources, string destination, TransferVerb verb) =>
        _transferPlanner.Plan(sources, destination, verb);

    /// <summary>Carries out a planned drop off the UI thread, then refreshes the tree nodes and
    /// every open tab on both sides of the transfer.</summary>
    /// <returns>What happened, or null when the drop was refused before it began — another
    /// transfer already running, or a plan with nothing in it.</returns>
    public async Task<TransferOutcome?> ExecuteDropAsync(
        TransferPlan plan, IReadOnlyDictionary<string, ConflictResolution>? resolutions)
    {
        if (IsTransferring || !plan.HasWork) return null;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();

        var cancellation = new CancellationTokenSource();
        try
        {
            // The byte total is a lookup, not a walk: dir_size_cache already holds the recursive
            // size of every directory on an indexed volume. Where it does not, the estimate comes
            // back incomplete and the surfaces show throughput without a percentage or an ETA.
            var estimate = await Task.Run(
                () => TransferEstimator.Estimate(plan, IndexedTransferSizeSource.For(plan, _dirSizes)));

            var surface = new TransferProgressViewModel(plan, estimate, cancellation.Cancel);
            TransferProgress = surface;
            SetStatus(surface.Headline);

            // Constructed here so it captures the UI dispatcher: the handler then touches the view
            // model directly from the executor's background thread.
            var progress = new Progress<TransferProgress>(surface.Apply);

            var (outcome, elevated) = await ElevateIfRefusedAsync(
                plan,
                await Task.Run(
                    () => _transferExecutor.Execute(plan, resolutions, cancellation.Token, progress)),
                resolutions);

            RetireUndoable();
            if (outcome.CanUndo)
            {
                _undoableTransfer = outcome;
                UndoDescription = $"Ctrl+Z: undo move of {outcome.Completed.Count:N0} item(s)";
            }

            await RefreshAfterTransferAsync(plan, outcome);
            SetStatus(DescribeOutcome(plan, outcome) + elevated);
            return outcome;
        }
        finally
        {
            TransferProgress = null;
            cancellation.Dispose();
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    // --- Trying again with an administrator token ---

    /// <summary>
    /// Offers a second, elevated pass over whatever Windows refused, and folds the result back into
    /// one outcome.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Called from inside each of the four operations, after the ordinary pass and <b>before</b>
    /// <see cref="RetireUndoable"/>. That position is forced rather than chosen: retiring claims the
    /// one-level undo slot and erases the previous operation's staged data, so a retry raised after
    /// it would need a second undo record and claiming that would commit a staging folder the user
    /// might still have wanted back.
    /// </para>
    /// <para>
    /// Written out four times rather than made generic over three delegates. The types differ, and
    /// four honest copies that contain no rules — every rule is in <see cref="ElevatedRetry"/> —
    /// read better than one clever one.
    /// </para>
    /// </remarks>
    private readonly record struct Elevated<T>(T Outcome, string Note);

    private async Task<Elevated<TransferOutcome>> ElevateIfRefusedAsync(
        TransferPlan plan,
        TransferOutcome outcome,
        IReadOnlyDictionary<string, ConflictResolution>? resolutions)
    {
        if (ElevatedRetry.RetryFor(plan, outcome, resolutions) is not { } retry)
            return new Elevated<TransferOutcome>(outcome, "");

        var verb = plan.Verb == TransferVerb.Move ? ElevationOperation.TransferMove : ElevationOperation.TransferCopy;
        if (!Asked(verb, outcome.Failed.Where(f => f.AccessDenied).Select(f => f.SourcePath), out var note))
            return new Elevated<TransferOutcome>(outcome, note);

        // Its own progress surface, built from the retry plan: reusing the first pass's would leave
        // its item counts and byte total describing an operation that has already finished.
        var estimate = TransferEstimator.Estimate(
            retry.Plan, IndexedTransferSizeSource.For(retry.Plan, _dirSizes));
        var surface = new TransferProgressViewModel(retry.Plan, estimate, () => { })
        {
            Headline = $"Retrying {retry.Plan.Transfers.Count:N0} item(s) as administrator…",
        };
        TransferProgress = surface;

        var run = await _elevation.RunAsync(retry, new Progress<TransferProgress>(surface.Apply));
        TransferProgress = null;

        return run.Result is { } second
            ? new Elevated<TransferOutcome>(ElevatedRetry.Merge(outcome, retry, second), "")
            : new Elevated<TransferOutcome>(outcome, Describe(run.Detail));
    }

    private async Task<Elevated<DeleteOutcome>> ElevateIfRefusedAsync(DeletePlan plan, DeleteOutcome outcome)
    {
        if (ElevatedRetry.RetryFor(plan, outcome) is not { } retry)
            return new Elevated<DeleteOutcome>(outcome, "");

        if (!Asked(ElevationOperation.Delete, outcome.Failed.Where(f => f.AccessDenied).Select(f => f.SourcePath), out var note))
            return new Elevated<DeleteOutcome>(outcome, note);

        var run = await _elevation.RunAsync(retry);
        return run.Result is { } second
            ? new Elevated<DeleteOutcome>(ElevatedRetry.Merge(outcome, retry, second), "")
            : new Elevated<DeleteOutcome>(outcome, Describe(run.Detail));
    }

    private async Task<Elevated<RenameOutcome>> ElevateIfRefusedAsync(RenamePlan plan, RenameOutcome outcome)
    {
        if (ElevatedRetry.RetryFor(plan, outcome) is not { } retry)
            return new Elevated<RenameOutcome>(outcome, "");

        if (!Asked(ElevationOperation.Rename, outcome.Failed.Where(f => f.AccessDenied).Select(f => f.SourcePath), out var note))
            return new Elevated<RenameOutcome>(outcome, note);

        var run = await _elevation.RunAsync(retry);
        return run.Result is { } second
            ? new Elevated<RenameOutcome>(ElevatedRetry.Merge(outcome, retry, second), "")
            : new Elevated<RenameOutcome>(outcome, Describe(run.Detail));
    }

    private async Task<Elevated<NewItemOutcome>> ElevateIfRefusedAsync(NewItemPlan plan, NewItemOutcome outcome)
    {
        if (ElevatedRetry.RetryFor(plan, outcome) is not { } retry)
            return new Elevated<NewItemOutcome>(outcome, "");

        if (!Asked(ElevationOperation.NewItem, [plan.TargetPath], out var note))
            return new Elevated<NewItemOutcome>(outcome, note);

        var run = await _elevation.RunAsync(retry);
        return run.Result is { } second
            ? new Elevated<NewItemOutcome>(ElevatedRetry.Merge(outcome, retry, second), "")
            : new Elevated<NewItemOutcome>(outcome, Describe(run.Detail));
    }

    /// <summary>
    /// Whether to go ahead: this account can elevate, and the user said yes.
    /// </summary>
    /// <remarks>
    /// <b>The account is checked before the dialog, never after.</b> A standard user shown a shield
    /// gets a credential prompt for somebody else's password — firing that at somebody who never
    /// asked for it is a good deal worse than quietly saying it cannot be done.
    /// </remarks>
    private bool Asked(
        ElevationOperation operation, IEnumerable<string> items, out string note, bool isUndo = false)
    {
        if (!_elevation.CanElevate)
        {
            note = " — and this account cannot provide administrator permission";
            return false;
        }

        note = "";
        return _elevationPrompt.Offer(new ElevationOffer(operation, [.. items], isUndo));
    }


    /// <summary>
    /// The same offer, for putting things back. Undoing a move that needed a token costs a second
    /// prompt, which is honest: restoring a file needs the rights taking it did.
    /// </summary>
    /// <remarks>
    /// It is not optional polish either. <c>ShellRecycleBin.Restore</c> invokes the shell's
    /// <c>undelete</c> verb, which puts an item back at its <em>original</em> path — the path that
    /// was refused in the first place, which is why it was elevated. So the undo of an elevated
    /// delete will usually need elevation too.
    /// </remarks>
    private async Task<Elevated<TransferUndoResult>> ElevateIfRefusedAsync(
        TransferOutcome outcome, TransferUndoResult result)
    {
        if (ElevatedRetry.UndoRetryFor(outcome, result) is not { } retry)
            return new Elevated<TransferUndoResult>(result, "");

        if (!Asked(ElevationOperation.TransferUndo,
                result.Failed.Where(f => f.AccessDenied).Select(f => f.SourcePath), out var note, isUndo: true))
            return new Elevated<TransferUndoResult>(result, note);

        var run = await _elevation.UndoAsync(retry.Outcome);
        return run.Result is { } second
            ? new Elevated<TransferUndoResult>(ElevatedRetry.Merge(result, retry, second), "")
            : new Elevated<TransferUndoResult>(result, Describe(run.Detail));
    }

    /// <inheritdoc cref="ElevateIfRefusedAsync(TransferOutcome, TransferUndoResult)"/>
    private async Task<Elevated<DeleteUndoResult>> ElevateIfRefusedAsync(
        DeleteOutcome outcome, DeleteUndoResult result)
    {
        if (ElevatedRetry.UndoRetryFor(outcome, result) is not { } retry)
            return new Elevated<DeleteUndoResult>(result, "");

        if (!Asked(ElevationOperation.DeleteUndo,
                result.Failed.Where(f => f.AccessDenied).Select(f => f.SourcePath), out var note, isUndo: true))
            return new Elevated<DeleteUndoResult>(result, note);

        var run = await _elevation.UndoAsync(retry.Outcome);
        return run.Result is { } second
            ? new Elevated<DeleteUndoResult>(ElevatedRetry.Merge(result, retry, second), "")
            : new Elevated<DeleteUndoResult>(result, Describe(run.Detail));
    }
    private static string Describe(string detail) => detail.Length == 0 ? "" : $" — {detail}";

    // --- Extracting ---

    /// <summary>Plans pulling entries out of the container the tab is showing.</summary>
    public ExtractPlan PlanExtract(
        DirectoryTabViewModel tab,
        IReadOnlyList<string> entryPaths,
        string destinationDirectory,
        ExtractConflict conflict)
    {
        if (_archives.Resolve(tab.CurrentPath) is not { } here)
            return ExtractPlan.Refused(
                ExtractRejection.ArchiveUnreadable, "This folder is not inside an archive.");

        var index = _archives.ReadArchive(here.ArchiveFile);

        // The entry paths arriving are full virtual paths — that is what a row's FullPath is — so
        // they are turned back into keys here rather than everywhere upstream.
        var keys = entryPaths
            .Select(p => _archives.Resolve(p)?.EntryPath)
            .Where(k => !string.IsNullOrEmpty(k))
            .Select(k => k!)
            .ToList();

        return _extractPlanner.Plan(
            index, here.ArchiveFile, here.EntryPath, keys, destinationDirectory, conflict);
    }

    /// <summary>The container a path is inside, if any — what the Unlock button needs to name.</summary>
    public string? ArchiveFileFor(string path) => _archives.Resolve(path)?.ArchiveFile;

    /// <summary>Holds a password for this session only. See <c>ArchivePasswordStore</c>.</summary>
    public void RememberArchivePassword(string archiveFile, string password)
    {
        if (_archivePasswords is Services.ArchivePasswordStore store)
            store.Remember(archiveFile, password);
    }

    /// <summary>Drops a password that turned out to be wrong, so it is not retried silently.</summary>
    public void ForgetArchivePassword(string archiveFile)
    {
        if (_archivePasswords is Services.ArchivePasswordStore store)
            store.Forget(archiveFile);
    }

    /// <summary>Where "Extract here" would put things, for seeding the dialog's destination box.</summary>
    public string SuggestExtractDestination(string archiveFile)
    {
        var index = _archives.ReadArchive(archiveFile);
        return ExtractRules.DestinationFor(index, archiveFile, Directory.Exists, File.Exists);
    }

    /// <summary>
    /// Carries out an extract off the UI thread, on the transfer progress surface.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Modelled on <see cref="ExecuteDropAsync"/> and sharing its whole surface: a synthetic
    /// <see cref="TransferPlan"/> feeds <see cref="TransferProgressViewModel"/>, which the
    /// status-bar strip and the detail window already bind to. Nothing in Transfer had to learn
    /// what an archive is, and nothing in the progress UI had to be duplicated.
    /// </para>
    /// <para>
    /// It claims <see cref="IsTransferring"/> like the others, so two of these cannot overlap and
    /// the undo slot stays coherent — but it never <em>writes</em> to that slot, because extracting
    /// is additive. Whatever was undoable before an extract is still undoable after it, and Delete
    /// removes what this made, reversibly.
    /// </para>
    /// </remarks>
    public async Task<ExtractOutcome?> ExecuteExtractAsync(ExtractPlan plan)
    {
        if (IsTransferring || !plan.HasWork) return null;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();

        var cancellation = new CancellationTokenSource();
        try
        {
            var files = plan.Items.Where(i => !i.IsDirectory).ToList();

            var synthetic = new TransferPlan(
                TransferVerb.Copy,
                plan.DestinationDirectory,
                files.Select(f => new PlannedTransfer(
                    f.EntryPath, IsDirectory: false, f.DestinationPath, Conflicts: false)).ToList(),
                []);

            // Exact for an addressable container, because the uncompressed lengths were already in
            // its directory — better than the filesystem case. A sequential one reports a floor,
            // and the bar goes indeterminate rather than lying about a percentage.
            var estimate = new TransferEstimate(plan.TotalBytes, files.Count, plan.BytesAreExact);

            var surface = new TransferProgressViewModel(synthetic, estimate, cancellation.Cancel);
            surface.Headline = $"Extracting {files.Count:N0} item(s)…";
            TransferProgress = surface;
            SetStatus(surface.Headline);

            var progress = new Progress<TransferProgress>(surface.Apply);
            var password = _archivePasswords.For(plan.ArchiveFile);

            var outcome = await Task.Run(
                () => _extractExecutor.Execute(plan, password, cancellation.Token, progress));

            await RefreshTabsShowingAsync([plan.DestinationDirectory]);
            await Tree.RefreshDirectoriesAsync([plan.DestinationDirectory]);
            SetStatus(DescribeExtract(outcome));
            return outcome;
        }
        finally
        {
            TransferProgress = null;
            cancellation.Dispose();
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    // --- Creating ---

    /// <summary>
    /// Compresses a selection into a new archive, on the transfer progress surface.
    /// </summary>
    /// <remarks>
    /// Additive like an extract, so it claims <see cref="IsTransferring"/> and never writes to the
    /// undo slot. The byte total is what the walk already measured — file lengths, not a
    /// <c>dir_size_cache</c> lookup — so it is exact, and the bar shows a real percentage.
    /// </remarks>
    public async Task<CreateArchiveOutcome?> ExecuteCreateArchiveAsync(
        IReadOnlyList<string> sources,
        string archivePath,
        ArchiveWriteFormat format,
        CompressionLevel level)
    {
        if (IsTransferring || sources.Count == 0) return null;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();

        var cancellation = new CancellationTokenSource();
        try
        {
            var collected = await Task.Run(
                () => ArchiveSourceWalk.Collect(sources, _settings.ShowHiddenItems, cancellation.Token));

            if (collected.Count == 0)
            {
                SetStatus("There is nothing to compress.");
                return null;
            }

            var synthetic = new TransferPlan(
                TransferVerb.Copy,
                Path.GetDirectoryName(archivePath) ?? "",
                collected.Select(s => new PlannedTransfer(
                    s.Path, IsDirectory: false, archivePath, Conflicts: false)).ToList(),
                []);

            var estimate = new TransferEstimate(
                collected.Sum(s => s.SizeBytes), collected.Count, Complete: true);

            var surface = new TransferProgressViewModel(synthetic, estimate, cancellation.Cancel);
            surface.Headline = $"Compressing {collected.Count:N0} item(s)…";
            TransferProgress = surface;
            SetStatus(surface.Headline);

            var progress = new Progress<TransferProgress>(surface.Apply);

            var outcome = await Task.Run(() => _archiveCreator.Create(
                archivePath, format, level, collected, cancellation.Token, progress));

            var folder = Path.GetDirectoryName(archivePath);
            if (folder is { Length: > 0 })
            {
                await RefreshTabsShowingAsync([folder]);
                await Tree.RefreshDirectoriesAsync([folder]);
            }

            SetStatus(outcome.Cancelled
                ? "Compress cancelled — nothing was written."
                : $"{Path.GetFileName(archivePath)} created from {outcome.FilesWritten:N0} file(s).");
            return outcome;
        }
        finally
        {
            TransferProgress = null;
            cancellation.Dispose();
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    // --- Editing a container ---

    /// <summary>Plans changing the contents of the archive the tab is standing in.</summary>
    public ArchiveEditPlan PlanArchiveEdit(
        DirectoryTabViewModel tab, IReadOnlyList<ArchiveEdit> edits)
    {
        if (_archives.Resolve(tab.CurrentPath) is not { } here)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Unreadable, "This folder is not inside an archive.");

        long bytes;
        try
        {
            bytes = new FileInfo(here.ArchiveFile).Length;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Unreadable, "The archive could not be opened.");
        }

        return _archiveEditPlanner.Plan(
            _archives.ReadArchive(here.ArchiveFile), here.ArchiveFile, bytes, edits);
    }

    /// <summary>
    /// The naming half of a rename, planned against the container rather than the disk.
    /// </summary>
    /// <remarks>
    /// Same planner, different probe — see <see cref="ArchiveRenameProbe"/>. The dialog previews
    /// with this, so what it shows is what the rewrite will produce.
    /// </remarks>
    public RenamePlan PlanRenameInArchive(IReadOnlyList<RenameSource> sources, RenameRule rule)
    {
        if (sources.Count == 0 || _archives.Resolve(sources[0].Path) is not { } here)
            return PlanRename(sources, rule);

        var probe = new ArchiveRenameProbe(_archives.ReadArchive(here.ArchiveFile), here.ArchiveFile);
        return new RenamePlanner(probe).Plan(sources, rule);
    }

    /// <summary>The entry path a virtual path names inside its container, or null.</summary>
    public string? ArchiveEntryPathFor(string path) => _archives.Resolve(path)?.EntryPath;

    /// <summary>Turns selected rows inside a container into the edits that would remove them.</summary>
    public IReadOnlyList<ArchiveEdit> RemovalsFor(IReadOnlyList<string> fullPaths) =>
        fullPaths
            .Select(p => _archives.Resolve(p)?.EntryPath)
            .Where(e => !string.IsNullOrEmpty(e))
            .Select(ArchiveEdit (e) => new RemoveEntry(e!))
            .ToList();

    /// <summary>
    /// Carries out an archive edit, on the transfer progress surface and the shared undo slot.
    /// </summary>
    /// <remarks>
    /// Unlike extracting and compressing, this one <em>is</em> destructive, so it takes the undo
    /// slot like a move, a rename or a delete — retiring whatever was there first, which is what
    /// commits the previous operation's staging and keeps exactly one thing undoable at a time.
    /// </remarks>
    public async Task<ArchiveEditOutcome?> ExecuteArchiveEditAsync(ArchiveEditPlan plan)
    {
        if (IsTransferring || !plan.HasWork) return null;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();

        var cancellation = new CancellationTokenSource();
        try
        {
            var name = Path.GetFileName(plan.ArchiveFile);

            var synthetic = new TransferPlan(
                TransferVerb.Copy, Path.GetDirectoryName(plan.ArchiveFile) ?? "",
                [new PlannedTransfer(plan.ArchiveFile, false, plan.ArchiveFile, false)], []);

            // The whole container is rewritten however small the change, so the bar measures the
            // archive rather than the edit — which is the honest figure and the surprising one.
            var estimate = new TransferEstimate(plan.RewriteBytes, 1, Complete: true);

            var surface = new TransferProgressViewModel(synthetic, estimate, cancellation.Cancel);
            surface.Headline = $"Rewriting {name}…";
            TransferProgress = surface;
            SetStatus(surface.Headline);

            var progress = new Progress<TransferProgress>(surface.Apply);
            var outcome = await Task.Run(
                () => _archiveEditExecutor.Execute(plan, cancellation.Token, progress));

            RetireUndoable();
            if (outcome.CanUndo)
            {
                _undoableArchiveEdit = outcome;
                UndoDescription = $"Ctrl+Z: undo changes to {name}";
            }

            var folder = Path.GetDirectoryName(plan.ArchiveFile);
            if (folder is { Length: > 0 })
            {
                await Tree.RefreshDirectoriesAsync([folder]);
                await RefreshTabsShowingAsync([folder]);
            }
            await RefreshTabsUnderAsync(plan.ArchiveFile);

            SetStatus(outcome switch
            {
                { Cancelled: true } => $"{name} was left unchanged.",
                { Failure: { } failure } => failure,
                _ => $"{name} updated.",
            });
            return outcome;
        }
        finally
        {
            TransferProgress = null;
            cancellation.Dispose();
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    /// <summary>
    /// Reloads every tab standing inside <paramref name="archiveFile"/>.
    /// </summary>
    /// <remarks>
    /// <c>RefreshTabsShowingAsync</c> matches a tab's folder exactly, which is no use here: a tab
    /// may be several levels down inside the container that just changed. Matched with
    /// <c>PathKey.IsUnder</c>, never string comparison, the way the rest of the fan-out is.
    /// </remarks>
    private async Task RefreshTabsUnderAsync(string archiveFile)
    {
        var key = PathKey.Canonicalize(archiveFile);

        foreach (var tab in AllTabs.ToList())
        {
            if (tab.CurrentPath.Length == 0) continue;

            var tabKey = PathKey.Canonicalize(tab.CurrentPath);
            if (tabKey != key && !PathKey.IsUnder(tabKey, key)) continue;

            await tab.RefreshViewAsync();
        }
    }

    private static string DescribeExtract(ExtractOutcome outcome)
    {
        var written = $"{outcome.FilesWritten:N0} file(s) extracted";
        if (outcome.Cancelled) return $"Extract cancelled — {written}.";
        if (outcome.Failed.Count > 0) return $"{written}, {outcome.Failed.Count:N0} failed.";
        return $"{written}.";
    }

    /// <summary>Reverses the last move, rename or delete — whichever the undo slot holds.</summary>
    [RelayCommand(CanExecute = nameof(CanUndo))]
    private async Task UndoAsync()
    {
        // At most one of these is ever set: each operation retires the slot before claiming it.
        if (_undoableRename is { } rename) await UndoRenameAsync(rename);
        else if (_undoableDelete is { } deletion) await UndoDeleteAsync(deletion);
        else if (_undoableTransfer is { } transfer) await UndoTransferAsync(transfer);
        else if (_undoableArchiveEdit is { } edit) await UndoArchiveEditAsync(edit);
    }

    private async Task UndoArchiveEditAsync(ArchiveEditOutcome outcome)
    {
        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            var failure = await Task.Run(() => _archiveEditExecutor.Undo(outcome));

            _undoableArchiveEdit = null;
            UndoDescription = "";

            // The whole container changed, so anything showing its inside is stale. Refreshed by
            // the archive's own path as well as its folder, because a tab may be standing in it.
            var folder = Path.GetDirectoryName(outcome.ArchiveFile);
            if (folder is { Length: > 0 })
            {
                await Tree.RefreshDirectoriesAsync([folder]);
                await RefreshTabsShowingAsync([folder]);
            }
            await RefreshTabsUnderAsync(outcome.ArchiveFile);

            SetStatus(failure ?? $"Put back {Path.GetFileName(outcome.ArchiveFile)}.");
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    private async Task UndoTransferAsync(TransferOutcome outcome)
    {
        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            SetStatus("Undoing…");
            var (result, elevated) = await ElevateIfRefusedAsync(
                outcome, await Task.Run(() => _transferExecutor.Undo(outcome)));

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

            SetStatus((result.Failed.Count == 0
                ? $"Undone — {result.Restored:N0} item(s) put back"
                : $"Put back {result.Restored:N0} item(s); {result.Failed.Count:N0} could not be restored — {result.Failed[0].Message}") + elevated);
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
        if (_undoableArchiveEdit is { } edit)
            ArchiveEditExecutor.CommitStaging(edit);
        _undoableTransfer = null;
        _undoableRename = null;
        _undoableDelete = null;
        _undoableArchiveEdit = null;
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
        // A cancelled transfer says so, and says what did get across: the alternative reads as a
        // transfer that quietly moved fewer items than it was asked to.
        var text = outcome.Cancelled
            ? $"{(plan.Verb == TransferVerb.Move ? "Move" : "Copy")} cancelled — " +
              $"{outcome.Completed.Count:N0} of {plan.Transfers.Count:N0} item(s) {verb.ToLowerInvariant()}"
            : $"{verb} {outcome.Completed.Count:N0} item(s)";
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

    /// <summary>Opens the file or folder in VS Code. Finds the editor behind the <c>code</c>
    /// launcher on PATH, then falls back to the standard user/system install locations of
    /// Code.exe, and only then to the launcher itself.</summary>
    /// <remarks>
    /// The launcher is deliberately the <em>last</em> candidate rather than the first, even though
    /// it is what PATH points at: <c>code</c> is a batch file, and starting a batch file through
    /// the shell puts a console window on screen beside the editor it opened. See
    /// <see cref="VSCodePath"/>. It stays on the list because a console window is better than no
    /// editor — an install whose layout nothing here recognises still opens.
    /// </remarks>
    public void OpenInVSCode(string fullPath, bool isDirectory)
    {
        // Each candidate is resolved rather than attempted: a launch through the shell reports
        // nothing back, so "is VS Code installed?" has to be answered before anything is handed
        // over. This is what the old catch-and-try-the-next-one did.
        var launcher = _launcher.Resolve("code");

        string?[] candidates =
        [
            VSCodePath.BehindLauncher(launcher, _launcher.Resolve),
            _launcher.Resolve(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Programs", "Microsoft VS Code", "Code.exe")),
            _launcher.Resolve(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                "Microsoft VS Code", "Code.exe")),
            launcher,
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
