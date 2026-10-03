using System.Windows;
using System.Windows.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Commands;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Commands;
using BertBrowser.Core.Services.NewItem;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// The command palette's place in the window: opening and closing it, and gathering what it can
/// run. The catalogue's commands come from the registry; everything else here is a <em>named
/// thing</em> — a workspace, a bookmark, a theme, a settings page — turned into a row from the
/// same data the sidebar and the settings pages already show.
/// </summary>
public partial class MainWindow
{
    private const string SettingsOpenReason = "Not while Settings is open.";

    private CommandPaletteViewModel? _palette;

    /// <summary>What had the keyboard when the palette opened, to give it back on the way out.</summary>
    private IInputElement? _focusBeforePalette;

    internal bool IsPaletteOpen => PaletteHost.Visibility == Visibility.Visible;

    /// <summary>The palette's view model while it is up; for the UI harness, which drives it the
    /// way the keyboard does.</summary>
    internal CommandPaletteViewModel? Palette => IsPaletteOpen ? _palette : null;

    /// <summary>
    /// Opens the palette, or — pressed again while it is up — closes it.
    /// </summary>
    /// <param name="seed">What the box starts with: nothing, or a path for "Go to folder".</param>
    internal void ShowPalette(string seed = "")
    {
        if (IsPaletteOpen)
        {
            ClosePalette();
            return;
        }

        if (_palette is null)
        {
            _palette = new CommandPaletteViewModel(App.Services.GetRequiredService<PaletteMemory>());
            _palette.CloseRequested += then => ClosePalette(then);
            _palette.ShortcutRequested += ShowKeyboardShortcut;
            PaletteHost.DataContext = _palette;

            // Like a menu, it does not outlive the attention that opened it: coming back to the
            // window to find a list still hanging over the folders would be a surprise.
            Deactivated += (_, _) => ClosePalette(restoreFocus: false);
        }

        _focusBeforePalette = Keyboard.FocusedElement;

        _palette.Open(
            BuildPaletteItems(),
            PaletteSuggestions.For(_shell.ActiveTab.SelectedItems.Count, IsSettingsOpen),
            PlacesFor,
            seed);

        PaletteHost.Visibility = Visibility.Visible;
        PaletteHost.FocusBox();
    }

    /// <summary>Closes the palette, hands the keyboard back, and then runs what was chosen — in
    /// that order, so a command that moves focus (the address bar, a pane) is moving it from where
    /// the user was rather than from a box that is about to vanish.</summary>
    /// <param name="restoreFocus">False when the window has just lost activation: focusing one
    /// of its elements then would pull activation straight back from wherever the user went.</param>
    internal void ClosePalette(Action? then = null, bool restoreFocus = true)
    {
        if (!IsPaletteOpen) return;

        PaletteHost.Visibility = Visibility.Collapsed;

        if (restoreFocus)
        {
            if (_focusBeforePalette is UIElement { IsVisible: true, IsEnabled: true } element) element.Focus();
            else if (IsSettingsOpen) _settingsView?.FocusCategories();
            else _layoutHost.ActivePaneView?.FocusActiveTabList();
        }
        _focusBeforePalette = null;

        then?.Invoke();
    }

    // --- What it can run ---

    private List<PaletteItem> BuildPaletteItems()
    {
        var items = new List<PaletteItem>();
        var tab = _shell.ActiveTab;

        // The catalogue, in its own order, which is the order the categories are listed in.
        foreach (var command in CommandCatalog.All)
        {
            var id = command.Id;
            // The palette is not offered as a row in itself.
            if (id == "app.palette") continue;

            var unavailable = IsSettingsOpen && command.Context != CommandContext.App
                ? SettingsOpenReason
                : Commands.Unavailable(id);

            items.Add(new PaletteItem(
                new PaletteEntry(
                    id, command.Name, command.Category, command.Icon, _keymap.GestureText(id),
                    command.Aliases, command.Prominence, unavailable),
                () => Commands.TryExecute(id),
                Bindable: true));
        }

        var blocked = IsSettingsOpen ? SettingsOpenReason : null;

        foreach (var workspace in _shell.SavedWorkspaces.Items)
        {
            items.Add(new PaletteItem(
                new PaletteEntry($"workspace:{workspace.Name}", workspace.Name, "Workspaces", "Icon.SplitRight",
                    Aliases: ["switch workspace", "layout", "tab set"], Unavailable: blocked, Detail: workspace.ShapeText),
                () => _ = _shell.SwitchWorkspaceAsync(workspace)));
        }

        foreach (var search in _shell.SavedSearches.Items)
        {
            items.Add(new PaletteItem(
                new PaletteEntry($"search:{search.Name}", search.Name, "Saved searches", "Icon.Search",
                    Aliases: ["run search"], Unavailable: blocked, Detail: search.ScopeColumnText),
                () => _ = _shell.RunSavedSearchAsync(search)));
        }

        foreach (var bookmark in _shell.Bookmarks.Items)
        {
            items.Add(new PaletteItem(
                new PaletteEntry($"bookmark:{bookmark.FullPath}", bookmark.Name, "Bookmarks", "Icon.Bookmark",
                    Aliases: ["favourite", "favorite", "go to"], Unavailable: blocked, Detail: bookmark.FullPath),
                () => _ = _shell.OpenBookmarkAsync(bookmark)));
        }

        AddCustomCommands(items, blocked);
        AddNewFileTypes(items, blocked);
        AddOpenTabs(items, blocked);
        AddDrives(items, blocked);
        AddColumns(items, tab, blocked);
        AddThemes(items);
        AddSettingsPages(items);

        return items;
    }

    /// <summary>
    /// The user's own commands. They act on the selection or — with nothing selected — on the
    /// folder being shown, which is the reading the right-click menu takes over empty space.
    /// </summary>
    private void AddCustomCommands(List<PaletteItem> items, string? blocked)
    {
        foreach (var command in _settings.CustomCommands)
        {
            var id = KeymapService.CustomId(command.Id);
            items.Add(new PaletteItem(
                new PaletteEntry(id, command.Name, "Custom commands",
                    command.RunElevated ? "Icon.Shield" : "Icon.CustomCommand",
                    _keymap.GestureText(id), Unavailable: blocked ?? CustomCommandUnavailable(command),
                    Detail: Path.GetFileName(command.Command)),
                () => _shell.RunCustomCommand(command, CustomCommandTargets()),
                Bindable: true));
        }
    }

    /// <summary>
    /// One of the user's own commands as the registry sees it, by the id the keymap binds it under
    /// — so a shortcut given to one runs through the same availability check everything else does.
    /// Looked up each time rather than held: the list is edited on the settings page.
    /// </summary>
    private CommandHandler? CustomCommandHandler(string id)
    {
        if (!id.StartsWith(KeymapService.CustomPrefix, StringComparison.Ordinal)) return null;

        var command = _settings.CustomCommands.FirstOrDefault(c => KeymapService.CustomId(c.Id) == id);
        return command is null
            ? null
            : new CommandHandler(
                () => _shell.RunCustomCommand(command, CustomCommandTargets()),
                () => CustomCommandUnavailable(command));
    }

    private List<(string FullPath, bool IsDirectory)> CustomCommandTargets()
    {
        var tab = _shell.ActiveTab;
        if (tab.SelectedItems.Count > 0)
            return [.. tab.SelectedItems.Select(i => (i.FullPath, i.IsDirectory))];
        return tab.CurrentPath.Length > 0 ? [(tab.CurrentPath, true)] : [];
    }

    /// <summary>Why one of the user's commands cannot run here, or null when it can.</summary>
    private string? CustomCommandUnavailable(CustomCommandDefinition command)
    {
        if (InArchive) return "Not available inside an archive.";

        var targets = CustomCommandTargets();
        if (targets.Any(t => t.IsDirectory ? command.AppliesToDirectories : command.AppliesToFiles)) return null;

        return (command.AppliesToFiles, command.AppliesToDirectories) switch
        {
            (true, false) => "It applies to files — select one.",
            (false, true) => "It applies to folders — select one, or clear the selection for this folder.",
            _ => "There is nothing here for it to act on.",
        };
    }

    private void AddNewFileTypes(List<PaletteItem> items, string? blocked)
    {
        var unavailable = blocked ?? WhyNot(FileVerb.New);
        foreach (var template in _settings.ResolvedNewFileTypes)
        {
            items.Add(new PaletteItem(
                new PaletteEntry($"new:{template.Label}|{template.Extension}", $"New {template.Label}", "New", "Icon.NewFile",
                    Aliases: ["create file", template.Extension], Unavailable: unavailable, Detail: template.Extension),
                () => ActiveTabView?.RunNew(NewItemKind.File, template)));
        }
    }

    private void AddOpenTabs(List<PaletteItem> items, string? blocked)
    {
        var number = 0;
        foreach (var pane in _shell.AllPanes)
        {
            foreach (var tab in pane.Tabs)
            {
                number++;
                var (target, home) = (tab, pane);
                items.Add(new PaletteItem(
                    new PaletteEntry($"tab:{number}", tab.Title, "Open tabs", "Icon.Tab",
                        Aliases: ["switch to tab"], Unavailable: blocked, Detail: tab.CurrentPath),
                    () =>
                    {
                        _shell.ActivatePane(home);
                        home.ActiveTab = target;
                        _layoutHost.ViewFor(home)?.FocusActiveTabList();
                    },
                    // The third tab today is a different tab tomorrow.
                    Remembered: false));
            }
        }
    }

    private void AddDrives(List<PaletteItem> items, string? blocked)
    {
        DriveInfo[] drives;
        try
        {
            drives = DriveInfo.GetDrives();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return;
        }

        foreach (var drive in drives)
        {
            var root = drive.Name;
            items.Add(new PaletteItem(
                new PaletteEntry($"drive:{root}", root, "Drives", "Icon.Drive",
                    Aliases: ["go to drive", "volume"], Unavailable: blocked, Detail: DriveLabel(drive)),
                () => GoTo(root),
                Completion: root));
        }
    }

    /// <summary>A drive's label, without waiting on one that is not there: asking an empty card
    /// reader for its label is what makes a list of drives take seconds to appear.</summary>
    private static string DriveLabel(DriveInfo drive)
    {
        try
        {
            if (drive.DriveType is DriveType.Network or DriveType.CDRom or DriveType.Removable) return drive.DriveType.ToString();
            return drive.IsReady ? drive.VolumeLabel : "";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return "";
        }
    }

    /// <summary>Every built-in and curated column, each as "show" or "hide" for this tab.</summary>
    private void AddColumns(List<PaletteItem> items, DirectoryTabViewModel tab, string? blocked)
    {
        var showing = ColumnLayoutRules.Normalize(tab.FileList.ColumnLayout)
            .Select(c => c.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        foreach (var spec in ColumnCatalog.BuiltIns.Concat(ColumnCatalog.Curated))
        {
            // Folder and Match come and go with the listing, and Name cannot be turned off.
            if (ColumnCatalog.IsInjected(spec.Id) || spec.Id == ColumnCatalog.Name) continue;

            var on = showing.Contains(spec.Id);
            var id = spec.Id;
            items.Add(new PaletteItem(
                new PaletteEntry($"column:{id}", $"{(on ? "Hide" : "Show")} column: {spec.Header}", "Columns",
                    "Icon.Columns", Aliases: [spec.Header], Prominence: CommandProminence.Rare, Unavailable: blocked),
                () => tab.FileList.ColumnLayout = ColumnLayoutRules.Toggle(tab.FileList.ColumnLayout, id, !on)));
        }
    }

    private void AddThemes(List<PaletteItem> items)
    {
        var themes = App.Services.GetRequiredService<IThemeService>();
        foreach (var theme in themes.Available)
        {
            var id = theme.Id;
            var current = string.Equals(id, themes.Current.Id, StringComparison.OrdinalIgnoreCase);
            items.Add(new PaletteItem(
                new PaletteEntry($"theme:{id}", theme.Name, "Themes", "Icon.Appearance",
                    Aliases: ["colour theme", "color theme", "switch theme"],
                    Detail: current ? "in use" : ""),
                () => themes.SelectTheme(id)));
        }
    }

    private void AddSettingsPages(List<PaletteItem> items)
    {
        foreach (var page in SettingsViewModel.AllCategories)
        {
            var category = page.Id;
            items.Add(new PaletteItem(
                new PaletteEntry($"settings:{category}", page.Name, "Settings", page.IconKey,
                    Aliases: ["settings page", "options", "preferences"]),
                () => ShowSettings(category)));
        }
    }

    // --- Places ---

    /// <summary>How many folders a typed path lists under it. A screenful: past that, typing
    /// another letter narrows faster than reading does.</summary>
    private const int MaxPlaces = 12;

    /// <summary>
    /// What a typed path offers, or null when the text is not a path and is therefore a search for
    /// a command.
    /// </summary>
    /// <remarks>
    /// This runs on a keystroke, on the UI thread, so it only ever asks about local drives. A
    /// share or a mapped drive is offered as typed and left for the navigation to find out about — a dead server
    /// would otherwise hold the palette, and the window with it, for the length of a network
    /// timeout on every letter.
    /// </remarks>
    /// <summary>A share, or a drive letter mapped to one — which blocks exactly as a share does
    /// when its server is gone. Asking the drive's type touches no network.</summary>
    private static bool IsRemote(string path)
    {
        if (path.StartsWith(@"\\", StringComparison.Ordinal)) return true;

        try
        {
            return Path.GetPathRoot(path) is { Length: > 0 } root && new DriveInfo(root).DriveType == DriveType.Network;
        }
        catch (ArgumentException)
        {
            return false;
        }
    }

    private IReadOnlyList<PaletteItem>? PlacesFor(string query)
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (GoToRules.Parse(query, Environment.GetEnvironmentVariable, home) is not { } target) return null;

        var blocked = IsSettingsOpen ? SettingsOpenReason : null;
        var places = new List<PaletteItem>();
        var remote = IsRemote(target.Path);

        if (remote || Directory.Exists(target.Path))
        {
            var path = target.Path;
            var name = LeafName(path);
            places.Add(Place($"Go to {name}", path, "Icon.Folder", blocked, () => GoTo(path), Slashed(path)));
            places.Add(Place($"Open {name} in a new tab", path, "Icon.OpenInNewTab", blocked,
                () => _shell.OpenInNewTab(path, activate: true)));
            places.Add(Place($"Open {name} in a new pane", path, "Icon.OpenInNewPane", blocked,
                () => _shell.OpenInNewPane(path, SplitOrientation.Vertical)));
        }
        else if (File.Exists(target.Path))
        {
            var file = target.Path;
            places.Add(Place($"Show {Path.GetFileName(file)} in its folder", file, "Icon.MoveToFolder", blocked,
                () => _ = _shell.ActiveTab.RevealFileAsync(file)));
        }

        if (!remote) places.AddRange(FoldersUnder(target, blocked));

        if (places.Count == 0)
        {
            places.Add(new PaletteItem(
                new PaletteEntry("goto:none", target.Path, "Go to", "Icon.Folder", Unavailable: "There is no such folder."),
                () => { }, Remembered: false));
        }

        return places;
    }

    /// <summary>The folders in <paramref name="target"/>'s folder whose names start with what
    /// has been typed of the last segment.</summary>
    private IEnumerable<PaletteItem> FoldersUnder(GoToTarget target, string? blocked)
    {
        List<string> folders;
        try
        {
            if (!Directory.Exists(target.Folder)) return [];

            folders = Directory.EnumerateDirectories(target.Folder, target.Prefix + "*")
                .Where(path => _settings.ShowHiddenItems || !IsHiddenDirectory(path))
                // The folder that was typed in full is already the first row.
                .Where(path => !path.Equals(target.Path, StringComparison.OrdinalIgnoreCase))
                .OrderBy(path => path, StringComparer.OrdinalIgnoreCase)
                .Take(MaxPlaces)
                .ToList();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return [];
        }

        return folders.Select(path =>
            Place(LeafName(path), "", "Icon.Folder", blocked, () => GoTo(path), Slashed(path)));
    }

    /// <param name="name">Short, so the rows of one folder can be told apart at a glance: the
    /// part of the path that differs is its last segment, which is exactly what a long path
    /// trimmed to fit would cut off.</param>
    private static PaletteItem Place(
        string name, string detail, string icon, string? blocked, Action run, string? completion = null) =>
        new(new PaletteEntry("goto:" + name, name, "Go to", icon, Unavailable: blocked, Detail: detail),
            run,
            // A place somebody typed their way to is not a command to offer again unasked.
            Remembered: false,
            Completion: completion);

    private static string Slashed(string path) => path.EndsWith('\\') ? path : path + "\\";

    /// <summary>A folder's own name, or the whole of a path that has none — a drive's root.</summary>
    private static string LeafName(string path) =>
        Path.GetFileName(path.TrimEnd('\\')) is { Length: > 0 } leaf ? leaf : path;

    // --- Keyboard shortcuts ---

    /// <summary>Opens the Keyboard settings page on one command, ready to take a new shortcut.</summary>
    private void ShowKeyboardShortcut(string commandId)
    {
        ShowSettings(SettingsCategory.Keyboard);
        if (_settingsView is not { } view) return;

        // Selected and already listening, so "change this command's shortcut" from the palette is
        // one press of the new keys away rather than a page to find it on.
        view.ViewModel.SelectKeyCommand(commandId);
        Dispatcher.BeginInvoke(view.FocusKeyRow, System.Windows.Threading.DispatcherPriority.Loaded);
        if (view.ViewModel.ChangeKeyCommand.CanExecute(null)) view.ViewModel.ChangeKeyCommand.Execute(null);
    }
}
