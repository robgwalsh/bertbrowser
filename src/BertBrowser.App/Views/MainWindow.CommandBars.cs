using System.ComponentModel;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Threading;
using BertBrowser.App.Services.Commands;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.CommandBars;
using BertBrowser.Core.Services.Commands;
using BertBrowser.Core.Services.NewItem;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// The menu bar and the toolbars: the arrangement <see cref="CommandBarService"/> holds, drawn.
/// Nothing here knows what any command does — an item is an id, and it is shown, greyed, ticked
/// and run by asking the registry, the same way the palette does.
/// </summary>
/// <remarks>
/// <para>
/// <b>One gate.</b> <see cref="WhyNotNow"/> is the answer to "can this run?" for a menu row, a
/// toolbar button and a palette row alike, and <see cref="RunFromBar"/> asks it again on the way
/// in: a toolbar button is only as fresh as its last refresh.
/// </para>
/// <para>
/// <b>Menus are built when opened; toolbars are kept in step.</b> A dropdown is rebuilt from the
/// arrangement each time its heading opens, so it cannot be stale. A toolbar is always on show, so
/// it is refreshed — once, coalesced, when the app goes idle — after anything that might have
/// changed what a command can do. Input covers most of it (<c>CommandManager.RequerySuggested</c>);
/// the shell's and the active tab's property changes cover what finishes by itself, such as a
/// transfer ending or a navigation landing.
/// </para>
/// <para>
/// <b>Alt belongs to WPF.</b> The menu bar is a real <see cref="Menu"/>, and its access keys and
/// menu mode run after <see cref="OnPreviewKeyDown"/> has had first refusal — so a chord the
/// keymap claims (Alt+D, or whatever somebody rebinds) still wins, with no handling written here.
/// </para>
/// </remarks>
public partial class MainWindow
{
    private CommandBarService _bars = null!;
    private BarSource _barSource = null!;
    private CommandBarToolbarHost _toolbarHost = null!;
    private CommandBarToolbar _titleToolbar = null!;
    private readonly List<(CommandBarToolbar Toolbar, Panel Panel)> _rowToolbars = [];

    /// <summary>Held in a field because <c>RequerySuggested</c> keeps only a weak reference to
    /// its handlers: a lambda nothing else holds is collected, and the toolbar goes quiet.</summary>
    private EventHandler? _requerySuggested;

    private bool _barRefreshPending;
    private DirectoryTabViewModel? _barsTab;

    /// <summary>The title bar's toolbar, for the UI harness.</summary>
    internal CommandBarToolbar TitleBarToolbar => _titleToolbar;

    /// <summary>The toolbars under the title bar, top to bottom, for the UI harness.</summary>
    internal IReadOnlyList<CommandBarToolbar> RowToolbars => [.. _rowToolbars.Select(r => r.Toolbar)];

    private void InitializeCommandBars(CommandBarService bars)
    {
        _bars = bars;
        _barSource = new BarSource(this);
        _toolbarHost = new CommandBarToolbarHost(id => RunFromBar(id), ListRows, BuildMenuElements);
        _titleToolbar = new CommandBarToolbar(TitleToolbar, _toolbarHost, from => OverflowRows(_bars.Current.TitleBar, from));

        RebuildCommandBars();

        _bars.Changed += (_, _) =>
        {
            RebuildCommandBars();
            // The settings page edits this arrangement in place; one recalled from a menu or the
            // palette while it is open has to arrive there too.
            _settingsView?.ViewModel.LoadBars();
        };
        _keymap.Changed += (_, _) => InvalidateBars();
        _shell.PropertyChanged += (_, _) => InvalidateBars();
        _requerySuggested = (_, _) => InvalidateBars();
        CommandManager.RequerySuggested += _requerySuggested;

        // What the title bar's toolbar may take is what the menu, the switchers and the search
        // field leave it — none of which depends on the toolbar, so fitting it cannot feed back.
        TitleBarRoot.SizeChanged += (_, _) => FitToolbars();
        MainMenuBar.SizeChanged += (_, _) => FitToolbars();
        WorkspaceSwitcher.SizeChanged += (_, _) => FitToolbars();
        WorkspaceSwitcher.IsVisibleChanged += (_, _) => FitToolbars();
        SavedSearchSwitcher.SizeChanged += (_, _) => FitToolbars();
        SavedSearchSwitcher.IsVisibleChanged += (_, _) => FitToolbars();
        ToolbarRows.SizeChanged += (_, _) => FitToolbars();
    }

    // --- The gate ---

    /// <summary>
    /// Why a command cannot run at this moment, or null when it can. The registry answers for the
    /// command itself; this adds what only the window knows — that a dialog is up, or that the
    /// settings page is standing where the folders were, which silences everything about them.
    /// </summary>
    internal string? WhyNotNow(string id)
    {
        if (IsContentBlocked) return "Not while a dialog is open.";

        // One of the user's own commands acts on the folders, like any browser command.
        var context = CommandCatalog.Find(id)?.Context ?? CommandContext.Browser;
        if (IsSettingsOpen && context != CommandContext.App) return SettingsOpenReason;

        return Commands.Unavailable(id);
    }

    /// <summary>Runs a command chosen from a menu or a toolbar, if it still can.</summary>
    internal bool RunFromBar(string id) => WhyNotNow(id) is null && Commands.TryExecute(id);

    protected override void OnContentBlockedChanged(bool blocked)
    {
        // The title bar stays live under a dialog so the window can still be moved and closed;
        // what we put in it must not.
        MainMenuBar.IsEnabled = !blocked;
        TitleToolbar.IsEnabled = !blocked;
        InvalidateBars();
    }

    /// <summary>Everything a bar asks about commands in one go is judged against one snapshot of
    /// the selection (see <see cref="VerbNow"/>).</summary>
    private T BarPass<T>(Func<T> ask)
    {
        _inBarPass = true;
        try
        {
            return ask();
        }
        finally
        {
            _inBarPass = false;
            _passSnapshot = null;
        }
    }

    private sealed class BarSource(MainWindow window) : IBarSource
    {
        public BarCommandState? Command(string id)
        {
            if (window.Commands.Find(id) is not { } handler || window.Describe(id) is not { } described) return null;

            return new BarCommandState(
                described.Name, described.Icon, window._keymap.GestureText(id), window.WhyNotNow(id),
                handler.Checked?.Invoke() ?? false, handler.Caption?.Invoke());
        }

        public bool ListHasRows(string listId) => window.ListCount(listId) > 0;
    }

    /// <summary>A command's name and icon: the catalogue's, or those of one of the user's own.</summary>
    private (string Name, string? Icon)? Describe(string id)
    {
        if (CommandCatalog.Find(id) is { } command) return (command.Name, command.Icon);

        return _settings.CustomCommands.FirstOrDefault(c => KeymapService.CustomId(c.Id) == id) is { } custom
            ? (custom.Name, custom.RunElevated ? "Icon.Shield" : "Icon.CustomCommand")
            : null;
    }

    // --- The menu bar ---

    private void RebuildCommandBars()
    {
        BuildMenuBar();

        _rowToolbars.Clear();
        ToolbarRows.Children.Clear();
        foreach (var row in _bars.Current.Rows)
        {
            var panel = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(6, 3, 6, 3) };
            ToolbarRows.Children.Add(panel);
            _rowToolbars.Add((new CommandBarToolbar(panel, _toolbarHost, from => OverflowRows(row, from)), panel));
        }
        ToolbarRows.Visibility = _rowToolbars.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        RefreshBars();
    }

    /// <summary>
    /// The headings only. Each one's rows are built when it opens — but it is given one row now,
    /// because a heading with no children has no dropdown and so never raises the event that
    /// would fill it.
    /// </summary>
    private void BuildMenuBar()
    {
        MainMenuBar.Items.Clear();

        var menus = _bars.Current.Menus;
        var captions = AccessKeys.Assign([.. menus.Select(m => m.Caption ?? "")]);
        for (var i = 0; i < menus.Count; i++)
        {
            var heading = new MenuItem { Header = AccessKeys.ToUnderscored(captions[i]), Tag = menus[i] };
            AutomationProperties.SetName(heading, AccessKeys.Strip(captions[i]));
            heading.Items.Add(new MenuItem { IsEnabled = false });
            heading.SubmenuOpened += MenuHeading_SubmenuOpened;
            MainMenuBar.Items.Add(heading);
        }
    }

    private void MenuHeading_SubmenuOpened(object sender, RoutedEventArgs e)
    {
        // The event bubbles: a submenu further down opening arrives here too.
        if (!ReferenceEquals(e.OriginalSource, sender) || sender is not MenuItem { Tag: BarItem menu } heading) return;

        heading.Items.Clear();
        foreach (var element in BuildMainMenuItems(menu)) heading.Items.Add(element);
        if (heading.Items.Count == 0) heading.Items.Add(new MenuItem { Header = "(empty)", IsEnabled = false });
    }

    /// <summary>One menu's dropdown as it would open right now.</summary>
    private List<FrameworkElement> BuildMainMenuItems(BarItem menu) =>
        BarPass(() => BuildMenuElements(CommandBarRules.Present(menu.Items ?? [], _barSource)));

    /// <summary>The dropdown of the menu with this name ("File"), or null when there is none.
    /// Internal so the harness can photograph the same rows a click would show.</summary>
    internal List<FrameworkElement>? BuildMainMenuItems(string caption) =>
        _bars.Current.Menus.FirstOrDefault(m =>
                string.Equals(AccessKeys.Strip(m.Caption ?? ""), caption, StringComparison.OrdinalIgnoreCase))
            is { } menu
            ? BuildMainMenuItems(menu)
            : null;

    /// <summary>The headings on the menu bar, in order, as they read. For the UI harness.</summary>
    internal IReadOnlyList<string> MainMenuCaptions =>
        [.. _bars.Current.Menus.Select(m => AccessKeys.Strip(m.Caption ?? ""))];

    private List<FrameworkElement> BuildMenuElements(IReadOnlyList<BarRow> rows)
    {
        var elements = new List<FrameworkElement>();
        foreach (var row in rows)
        {
            switch (row.Kind)
            {
                case BarRowKind.Separator:
                    elements.Add(new Separator());
                    break;

                case BarRowKind.Command:
                {
                    var id = row.Id!;
                    var item = new MenuItem
                    {
                        Header = AccessKeys.ToUnderscored(row.Caption),
                        InputGestureText = row.Gesture,
                        IsEnabled = row.IsEnabled,
                        IsChecked = row.IsChecked,
                        // Greyed, it says why — the palette's rule, in a menu.
                        ToolTip = row.IsEnabled ? null : row.Reason,
                    };
                    if (row.Icon is { } icon) item.Icon = MenuIcon(icon);
                    ToolTipService.SetShowOnDisabled(item, true);
                    item.Click += (_, _) => RunFromBar(id);
                    elements.Add(item);
                    break;
                }

                case BarRowKind.Menu:
                {
                    var submenu = new MenuItem { Header = AccessKeys.ToUnderscored(row.Caption) };
                    foreach (var child in BuildMenuElements(row.Children)) submenu.Items.Add(child);
                    elements.Add(submenu);
                    break;
                }

                case BarRowKind.List:
                    elements.AddRange(ListRows(row.Id!));
                    break;

                case BarRowKind.Placeholder:
                    elements.Add(new MenuItem { Header = AccessKeys.ToUnderscored(row.Caption), IsEnabled = false });
                    break;
            }
        }
        return elements;
    }

    // --- Lists ---

    private int ListCount(string listId) => listId switch
    {
        CommandBarLists.Workspaces => _shell.SavedWorkspaces.Items.Count,
        CommandBarLists.SavedSearches => _shell.SavedSearches.Items.Count,
        CommandBarLists.Bookmarks => _shell.Bookmarks.Items.Count,
        CommandBarLists.NewFileTypes => _settings.ResolvedNewFileTypes.Count,
        CommandBarLists.CustomCommands => _settings.CustomCommands.Count,
        CommandBarLists.CommandBars => _bars.Saved.Count,
        // Always at least the one the app is running from; not worth a disk call to count.
        CommandBarLists.Drives => 1,
        CommandBarLists.UndoSteps => _shell.History.Undoable.Count,
        CommandBarLists.RedoSteps => _shell.History.Redoable.Count,
        _ => 0,
    };

    /// <summary>What a list is at this moment, as menu rows.</summary>
    private List<FrameworkElement> ListRows(string listId)
    {
        var rows = listId switch
        {
            CommandBarLists.Workspaces => WorkspaceRows(),
            CommandBarLists.SavedSearches => SavedSearchRows(),
            CommandBarLists.Bookmarks => BookmarkRows(),
            CommandBarLists.NewFileTypes => NewFileTypeRows(),
            // These are commands like any other, so they go through the same presentation — and
            // the same gate — as one placed by hand.
            CommandBarLists.CustomCommands => BuildMenuElements(CommandBarRules.Present(
                [.. _settings.CustomCommands.Select(c => BarItem.Command(KeymapService.CustomId(c.Id)))], _barSource)),
            CommandBarLists.CommandBars => CommandBarRows(),
            CommandBarLists.Drives => DriveRows(),
            CommandBarLists.UndoSteps => BuildUndoMenuItems(),
            CommandBarLists.RedoSteps => BuildRedoMenuItems(),
            _ => [],
        };

        // Everything a list offers acts on the folders, except recalling an arrangement — which is
        // exactly what somebody on the Menus and toolbars page wants to do.
        if (IsSettingsOpen && listId is not (CommandBarLists.CommandBars or CommandBarLists.CustomCommands))
        {
            foreach (var row in rows.OfType<MenuItem>().Where(r => r.IsEnabled))
            {
                row.IsEnabled = false;
                row.ToolTip = SettingsOpenReason;
                ToolTipService.SetShowOnDisabled(row, true);
            }
        }

        return rows;
    }

    private List<FrameworkElement> BookmarkRows()
    {
        var rows = new List<FrameworkElement>();
        foreach (var bookmark in _shell.Bookmarks.Items)
        {
            var row = new MenuItem { Header = Plain(bookmark.Name), ToolTip = bookmark.FullPath, Icon = MenuIcon("Icon.Bookmark") };
            row.Click += (_, _) => _ = _shell.OpenBookmarkAsync(bookmark);
            rows.Add(row);
        }
        return rows;
    }

    private List<FrameworkElement> NewFileTypeRows()
    {
        var whyNot = WhyNot(FileVerb.New);
        var rows = new List<FrameworkElement>();
        foreach (var template in _settings.ResolvedNewFileTypes)
        {
            var row = new MenuItem
            {
                Header = Plain(template.Label),
                InputGestureText = template.Extension,
                Icon = MenuIcon("Icon.NewFile"),
                IsEnabled = whyNot is null,
                ToolTip = whyNot,
            };
            ToolTipService.SetShowOnDisabled(row, true);
            row.Click += (_, _) => ActiveTabView?.RunNew(NewItemKind.File, template);
            rows.Add(row);
        }
        return rows;
    }

    private List<FrameworkElement> DriveRows()
    {
        DriveInfo[] drives;
        try
        {
            drives = DriveInfo.GetDrives();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return [];
        }

        var rows = new List<FrameworkElement>();
        foreach (var drive in drives)
        {
            var root = drive.Name;
            var row = new MenuItem { Header = Plain(root), InputGestureText = DriveLabel(drive), Icon = MenuIcon("Icon.Drive") };
            row.Click += (_, _) => GoTo(root);
            rows.Add(row);
        }
        return rows;
    }

    /// <summary>The saved arrangements, the one in force ticked.</summary>
    private List<FrameworkElement> CommandBarRows()
    {
        var current = _bars.CurrentName;
        var rows = new List<FrameworkElement>();
        foreach (var saved in _bars.Saved)
        {
            var name = saved.Name;
            var row = new MenuItem
            {
                Header = Plain(name),
                InputGestureText = SavedCommandBarRules.ShapeText(saved.Set),
                IsChecked = string.Equals(name, current, StringComparison.OrdinalIgnoreCase),
            };
            row.Click += (_, _) => ApplyCommandBars(name);
            rows.Add(row);
        }
        return rows;
    }

    /// <summary>A name from data as a menu header: an underscore in it is an underscore, not the
    /// mark of an access key.</summary>
    private static string Plain(string text) => text.Replace("_", "__");

    // --- Toolbars ---

    /// <summary>Asks for the toolbars to be brought up to date once the app is next idle. Called
    /// freely — any number of calls before then are one refresh.</summary>
    private void InvalidateBars()
    {
        if (_barRefreshPending) return;

        _barRefreshPending = true;
        Dispatcher.BeginInvoke(RefreshBars, DispatcherPriority.ContextIdle);
    }

    private void RefreshBars()
    {
        _barRefreshPending = false;
        WatchActiveTab();

        BarPass(() =>
        {
            ShowToolbar(_titleToolbar, _bars.Current.TitleBar);
            for (var i = 0; i < _rowToolbars.Count && i < _bars.Current.Rows.Count; i++)
                ShowToolbar(_rowToolbars[i].Toolbar, _bars.Current.Rows[i]);
            return true;
        });

        FitToolbars();
    }

    private void ShowToolbar(CommandBarToolbar toolbar, ToolbarSpec spec) =>
        toolbar.Show(CommandBarRules.Present(spec.Items, _barSource, accessKeys: false, showText: spec.ShowText));

    /// <summary>
    /// What changes by itself on the tab a toolbar speaks for — a listing arriving makes Back
    /// possible, a search finishing makes Stop impossible — is not input, so nothing else would
    /// say so. Followed tab by tab rather than subscribed for ever, since tabs come and go.
    /// </summary>
    private void WatchActiveTab()
    {
        var tab = _shell.ActiveTab;
        if (ReferenceEquals(tab, _barsTab)) return;

        if (_barsTab is not null)
        {
            _barsTab.PropertyChanged -= BarsTab_PropertyChanged;
            _barsTab.FileList.PropertyChanged -= BarsTab_PropertyChanged;
        }

        _barsTab = tab;
        tab.PropertyChanged += BarsTab_PropertyChanged;
        tab.FileList.PropertyChanged += BarsTab_PropertyChanged;
    }

    private void BarsTab_PropertyChanged(object? sender, PropertyChangedEventArgs e) => InvalidateBars();

    /// <summary>The toolbar's hidden tail as menu rows, for its overflow button. Presented afresh,
    /// as a menu, and cut at the same place — the two presentations hide the same items.</summary>
    private List<FrameworkElement> OverflowRows(ToolbarSpec spec, int from) =>
        BarPass(() =>
        {
            var rows = CommandBarRules.Present(spec.Items, _barSource).Skip(from)
                .SkipWhile(r => r.Kind == BarRowKind.Separator)
                .ToList();
            return BuildMenuElements(rows);
        });

    private void FitToolbars()
    {
        if (_titleToolbar is null) return;

        _titleToolbar.Fit(TitleToolbarRoom());
        foreach (var (toolbar, panel) in _rowToolbars)
            toolbar.Fit(ToolbarRows.ActualWidth > 0 ? ToolbarRows.ActualWidth - panel.Margin.Left - panel.Margin.Right : double.PositiveInfinity);
    }

    /// <summary>
    /// How wide the title bar's toolbar may be: the bar, less the menu, less the switchers that
    /// are showing, less the search group at its natural width. Infinite before the first layout,
    /// when nothing has a width to subtract.
    /// </summary>
    private double TitleToolbarRoom()
    {
        if (TitleBarRoot.ActualWidth <= 0) return double.PositiveInfinity;

        static double Taken(FrameworkElement element) =>
            element.IsVisible ? element.ActualWidth + element.Margin.Left + element.Margin.Right : 0;

        // The group's children are measured unconstrained, so their desired widths are what the
        // group wants even when the title bar has squeezed the group itself.
        var search = TitleBarSearchGroup.Children.Cast<UIElement>().Sum(c => c.DesiredSize.Width);

        return TitleBarRoot.ActualWidth
               - Taken(MainMenuBar) - Taken(WorkspaceSwitcher) - Taken(SavedSearchSwitcher)
               - search - TitleToolbar.Margin.Left - TitleToolbar.Margin.Right;
    }

    // --- Saved arrangements ---

    private void SaveCommandBars()
    {
        var seed = _bars.CurrentName ?? SavedCommandBarRules.DefaultName(DateTime.Now);
        if (SavedWorkspaceDialog.Show(this, seed, n => _bars.IsNameTaken(n), thing: NamedThing.CommandBars) is not { } name)
            return;

        _bars.SaveAs(name);
        _shell.SetStatus($"Saved the menus and toolbars as \"{name}\"");
    }

    /// <summary>
    /// Recalls a saved arrangement — asking first only when the one in force is kept nowhere else,
    /// since that is the only time anything would be lost.
    /// </summary>
    internal void ApplyCommandBars(string name)
    {
        if (_bars.ReplacingLosesWork && !ConfirmDiscardCommandBars($"\"{name}\"")) return;

        if (_bars.Apply(name)) _shell.SetStatus($"Menus and toolbars: {name}");
    }

    /// <summary>The one question about losing an arrangement, for the menu, the palette and the
    /// settings page alike.</summary>
    /// <param name="replacement">What would take its place, as it reads mid-sentence.</param>
    private bool ConfirmDiscardCommandBars(string replacement) =>
        App.Services.GetRequiredService<IUserConfirm>().Ask(
            $"The menus and toolbars have changes that are not saved under any name. Switch to {replacement} anyway?",
            "Menus and toolbars", "Switch");

    /// <summary>A saved arrangement's rename, from the settings page.</summary>
    private void RenameCommandBars(string name)
    {
        if (SavedWorkspaceDialog.Show(
                this, name, n => _bars.IsNameTaken(n, except: name), editingName: name, thing: NamedThing.CommandBars)
            is { } renamed)
        {
            _bars.Rename(name, renamed);
        }
    }

    private void ShowAbout() => AboutDialog.Show(this);
}
