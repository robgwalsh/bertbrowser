using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.ViewModels;

public enum MenuRowKind
{
    BuiltIn,
    Separator,
    Command,
    Shell,

    /// <summary>Where entries nobody has placed yet appear — see <see cref="MenuLayoutRules.More"/>.</summary>
    More,
}

/// <summary>
/// One line of the Context menu page's preview of the menu: an entry, a separator, or the place
/// new entries arrive. Its <see cref="Token"/> is what <c>AppSettings.ContextMenuLayout</c> stores.
/// </summary>
public sealed partial class MenuRowViewModel : ObservableObject
{
    private MenuRowViewModel(MenuRowKind kind, string token, string name)
    {
        Kind = kind;
        Token = token;
        _name = name;
    }

    public static MenuRowViewModel ForBuiltIn(BuiltInMenuItem item) =>
        new(MenuRowKind.BuiltIn, MenuLayoutRules.BuiltInToken(item.Id), item.Name)
        {
            IconKey = item.Icon,
            Gesture = item.Gesture ?? "",
            Detail = PlacesNote(item),
        };

    /// <summary>"file list only" for an entry one menu lacks; nothing for one both have.</summary>
    internal static string PlacesNote(BuiltInMenuItem item) =>
        item.Places == BuiltInMenuPlaces.Both ? "" : BuiltInMenuItems.PlacesText(item.Places) + " only";

    /// <summary>A new instance each time: a list shows one row per item, so two separators that
    /// were the same object would be one row.</summary>
    public static MenuRowViewModel Separator() => new(MenuRowKind.Separator, MenuLayoutRules.SeparatorToken, "");

    public static MenuRowViewModel ForMore() =>
        new(MenuRowKind.More, MenuLayoutRules.More, "New entries from other programs appear here");

    public static MenuRowViewModel ForCommand(CustomCommandItemViewModel command)
    {
        var row = new MenuRowViewModel(MenuRowKind.Command, MenuLayoutRules.CommandToken(command.Id), command.Name)
        {
            Command = command,
            IconKey = CommandIcon(command),
            Detail = "your command",
        };
        command.PropertyChanged += row.OnCommandChanged;
        return row;
    }

    /// <summary>An extension's row; the name is its id until the catalog scan has answered.</summary>
    public static MenuRowViewModel ForShell(string id, string? name = null, string detail = "") =>
        new(MenuRowKind.Shell, id, name ?? id) { Detail = detail };

    public MenuRowKind Kind { get; }

    public string Token { get; }

    /// <summary>The command behind a command row, which the editor edits; null otherwise.</summary>
    public CustomCommandItemViewModel? Command { get; private init; }

    [ObservableProperty]
    private string _name;

    /// <summary>What the row says beside its name: which menu only, "your command", a program's
    /// kind of entry.</summary>
    [ObservableProperty]
    private string _detail = "";

    public string Gesture { get; private init; } = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Icon))]
    private string? _iconKey;

    /// <summary>The outline beside the name, or null — other programs draw their own, which the
    /// page cannot see without loading them.</summary>
    public Geometry? Icon => IconKey is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;

    public bool IsSeparator => Kind == MenuRowKind.Separator;

    public bool IsMore => Kind == MenuRowKind.More;

    /// <summary>Everything but the place new entries arrive, which the menu always has.</summary>
    public bool IsRemovable => Kind != MenuRowKind.More;

    /// <summary>Fills in a shell row once the catalog scan has answered.</summary>
    internal void Describe(ShellExtension? extension)
    {
        if (extension is not null) Name = extension.Name;
        Detail = extension is null ? "not found on this computer" : ShellDetail(extension);
    }

    private void OnCommandChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (Command is null) return;
        if (e.PropertyName == nameof(CustomCommandItemViewModel.Name)) Name = Command.Name;
        else if (e.PropertyName == nameof(CustomCommandItemViewModel.RunElevated)) IconKey = CommandIcon(Command);
    }

    private static string CommandIcon(CustomCommandItemViewModel command) =>
        command.RunElevated ? "Icon.Shield" : "Icon.CustomCommand";

    internal static string ShellDetail(ShellExtension? extension) => extension?.Kind switch
    {
        ShellExtensionKind.Handler => "extension",
        ShellExtensionKind.Verb => "command",
        _ => "",
    };
}

/// <summary>One thing the Context menu page offers to put on the menu: one of the app's own entries
/// or another program's. Whether it is on the menu follows the preview.</summary>
public sealed partial class MenuChoiceViewModel : ObservableObject
{
    public MenuChoiceViewModel(string id, string token, string name, string? iconKey, string detail)
    {
        Id = id;
        Token = token;
        Name = name;
        IconKey = iconKey;
        Detail = detail;
    }

    /// <summary>The id the hidden list stores.</summary>
    public string Id { get; }

    public string Token { get; }

    public string Name { get; }

    public string? IconKey { get; }

    public string Detail { get; }

    public Geometry? Icon => IconKey is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;

    [ObservableProperty]
    private bool _isInMenu;
}

/// <summary>
/// The Context menu page: a preview of the menu on the left, in the order it will appear, and
/// everything that could be on it on the right.
/// </summary>
/// <remarks>
/// <para>
/// The preview is the one source of truth while the page is open. Whether an entry is "on the menu"
/// is whether it has a row there, and the hidden lists, each command's <c>ShowInMenu</c> and the
/// layout are all written from it by <see cref="ApplyMenuLayout"/>. That is what keeps the page and
/// the menu from disagreeing: there is no tick box that could say one thing while the row said
/// another.
/// </para>
/// <para>
/// Unplaced entries (commands made before the menu could be arranged, extensions nobody has placed)
/// are shown where the menu would show them — just above the "new entries" row — so the preview is
/// the real menu rather than a menu with a mystery row in it. Placing them there is the order they
/// already had, so doing so changes nothing until the user moves something.
/// </para>
/// </remarks>
public sealed partial class SettingsViewModel
{
    private readonly IShellMenuSource? _shellMenus;

    /// <summary>True while rows arrive from the catalog scan rather than from the user, so that
    /// finishing a scan is not itself a change to save.</summary>
    private bool _syncingMenu;

    /// <summary>The menu as it will appear, top to bottom.</summary>
    public ObservableCollection<MenuRowViewModel> MenuRows { get; } = [];

    [ObservableProperty]
    private MenuRowViewModel? _selectedMenuRow;

    /// <summary>Every one of the app's own entries, for the right-hand column.</summary>
    public ObservableCollection<MenuChoiceViewModel> BuiltInChoices { get; } = [];

    /// <summary>Every extension found on this machine. Filled after construction: the scan walks all
    /// of HKEY_CLASSES_ROOT and runs off the UI thread.</summary>
    public ObservableCollection<MenuChoiceViewModel> ShellChoices { get; } = [];

    /// <summary>The master switch; see <c>AppSettings.ShowShellExtensions</c>.</summary>
    [ObservableProperty]
    private bool _showShellExtensions;

    /// <summary>"Looking for extensions…" while the scan runs, a note when it finds none, else blank.</summary>
    [ObservableProperty]
    private string _shellExtensionsStatus = "";

    private void LoadMenuLayout()
    {
        var hiddenBuiltIns = new HashSet<string>(_settings.HiddenBuiltInMenuItems, StringComparer.OrdinalIgnoreCase);
        var hiddenShell = new HashSet<string>(_settings.HiddenShellExtensions, StringComparer.OrdinalIgnoreCase);

        foreach (var item in BuiltInMenuItems.All)
        {
            BuiltInChoices.Add(new MenuChoiceViewModel(
                item.Id, MenuLayoutRules.BuiltInToken(item.Id), item.Name, item.Icon,
                MenuRowViewModel.PlacesNote(item)));
        }

        var commands = Commands.ToDictionary(c => c.Id, StringComparer.OrdinalIgnoreCase);
        foreach (var token in MenuLayoutRules.Normalize(_settings.ContextMenuLayout, hiddenBuiltIns))
        {
            if (MenuLayoutRules.IsSeparator(token))
                MenuRows.Add(MenuRowViewModel.Separator());
            else if (token.Equals(MenuLayoutRules.More, StringComparison.OrdinalIgnoreCase))
                MenuRows.Add(MenuRowViewModel.ForMore());
            else if (MenuLayoutRules.IsBuiltIn(token, out var id))
            {
                if (!hiddenBuiltIns.Contains(id) && BuiltInMenuItems.Find(id) is { } item)
                    MenuRows.Add(MenuRowViewModel.ForBuiltIn(item));
            }
            else if (MenuLayoutRules.IsCommand(token, out var commandId))
            {
                if (commands.TryGetValue(commandId, out var command) && command.IsInMenu)
                    MenuRows.Add(MenuRowViewModel.ForCommand(command));
            }
            else if (!hiddenShell.Contains(token))
            {
                MenuRows.Add(MenuRowViewModel.ForShell(token));
            }
        }

        PlaceUnplaced(Commands.Where(c => c.IsInMenu && !HasRow(MenuLayoutRules.CommandToken(c.Id)))
            .Select(MenuRowViewModel.ForCommand));

        MenuRows.CollectionChanged += (_, _) => SyncMenuMembership();
        SyncMenuMembership();
    }

    private async Task LoadShellCatalogAsync()
    {
        if (_shellMenus is null) return;

        ShellExtensionsStatus = "Looking for extensions…";
        var catalog = await _shellMenus.CatalogAsync();
        var byId = catalog.ToDictionary(e => e.Id, StringComparer.OrdinalIgnoreCase);
        var hidden = new HashSet<string>(_settings.HiddenShellExtensions, StringComparer.OrdinalIgnoreCase);

        _syncingMenu = true;
        try
        {
            foreach (var extension in catalog)
            {
                ShellChoices.Add(new MenuChoiceViewModel(
                    extension.Id, extension.Id, extension.Name, null,
                    MenuRowViewModel.ShellDetail(extension)));
            }

            foreach (var row in MenuRows.Where(r => r.Kind == MenuRowKind.Shell))
                row.Describe(byId.GetValueOrDefault(row.Token));

            PlaceUnplaced(catalog.Where(e => !hidden.Contains(e.Id) && !HasRow(e.Id))
                .Select(e => MenuRowViewModel.ForShell(e.Id, e.Name, MenuRowViewModel.ShellDetail(e))));
            SyncMenuMembership();
        }
        finally
        {
            _syncingMenu = false;
        }

        ShellExtensionsStatus = catalog.Count == 0
            ? "No program on this computer adds right-click entries."
            : "";
    }

    /// <summary>Shows entries nobody has placed where the menu shows them: behind a separator, just
    /// above the "new entries" row.</summary>
    private void PlaceUnplaced(IEnumerable<MenuRowViewModel> rows)
    {
        var list = rows.ToList();
        if (list.Count == 0) return;

        var at = MoreIndex();
        MenuRows.Insert(at++, MenuRowViewModel.Separator());
        foreach (var row in list) MenuRows.Insert(at++, row);
    }

    private int MoreIndex()
    {
        for (var i = 0; i < MenuRows.Count; i++)
        {
            if (MenuRows[i].IsMore) return i;
        }
        return MenuRows.Count;
    }

    private bool HasRow(string token) =>
        MenuRows.Any(r => r.Token.Equals(token, StringComparison.OrdinalIgnoreCase));

    private void SyncMenuMembership()
    {
        var tokens = new HashSet<string>(MenuRows.Select(r => r.Token), StringComparer.OrdinalIgnoreCase);
        foreach (var choice in BuiltInChoices) choice.IsInMenu = tokens.Contains(choice.Token);
        foreach (var choice in ShellChoices) choice.IsInMenu = tokens.Contains(choice.Token);
        foreach (var command in Commands) command.IsInMenu = tokens.Contains(MenuLayoutRules.CommandToken(command.Id));
    }

    /// <summary>A command row selected in the preview is the command the editor shows.</summary>
    partial void OnSelectedMenuRowChanged(MenuRowViewModel? value)
    {
        if (value?.Command is { } command) SelectedCommand = command;
    }

    /// <summary>Where an added entry goes: under the selected row, or else just above the "new
    /// entries" row, which is where it would have appeared by itself.</summary>
    private int InsertionIndex() =>
        SelectedMenuRow is { } selected && MenuRows.IndexOf(selected) is >= 0 and var at ? at + 1 : MoreIndex();

    /// <summary>Moves a row — what a drag in the preview reports.</summary>
    public void MoveMenuRow(int from, int to)
    {
        if (from < 0 || from >= MenuRows.Count) return;
        to = Math.Clamp(to, 0, MenuRows.Count - 1);
        if (from == to) return;
        var row = MenuRows[from];
        MenuRows.Move(from, to);
        SelectedMenuRow = row;
    }

    /// <summary>Alt+Up and Alt+Down: the keyboard's half of the drag.</summary>
    public void NudgeSelectedMenuRow(int delta)
    {
        if (SelectedMenuRow is not { } selected) return;
        var from = MenuRows.IndexOf(selected);
        MoveMenuRow(from, from + delta);
    }

    /// <summary>
    /// Puts an entry from the right-hand column into the preview at a gap (0 is above the first
    /// row) — what a drag across reports. Dragging one that is already on the menu moves it.
    /// </summary>
    public void InsertMenuChoice(object choice, int gap)
    {
        if (RowFor(choice) is not { } row) return;

        gap = Math.Clamp(gap, 0, MenuRows.Count);
        var existing = MenuRows.IndexOf(row);
        if (existing >= 0)
        {
            MoveMenuRow(existing, gap > existing ? gap - 1 : gap);
            return;
        }

        MenuRows.Insert(gap, row);
        SelectedMenuRow = row;
    }

    /// <summary>The right-hand column's Add and Remove, one button whose meaning follows
    /// whether the entry is on the menu.</summary>
    [RelayCommand]
    private void ToggleMenuChoice(object? choice)
    {
        if (choice is null || RowFor(choice) is not { } row) return;

        if (MenuRows.Contains(row))
        {
            RemoveMenuRow(row);
            return;
        }

        MenuRows.Insert(InsertionIndex(), row);
        SelectedMenuRow = row;
    }

    [RelayCommand]
    private void RemoveMenuRow(MenuRowViewModel? row)
    {
        if (row is not { IsRemovable: true }) return;

        var index = MenuRows.IndexOf(row);
        if (index < 0) return;
        MenuRows.RemoveAt(index);
        if (SelectedMenuRow is null || SelectedMenuRow == row)
            SelectedMenuRow = MenuRows.Count > 0 ? MenuRows[Math.Min(index, MenuRows.Count - 1)] : null;
    }

    [RelayCommand]
    private void AddMenuSeparator()
    {
        var row = MenuRowViewModel.Separator();
        MenuRows.Insert(InsertionIndex(), row);
        SelectedMenuRow = row;
    }

    /// <summary>
    /// The app's own entries back in their original order and all on the menu. The user's commands
    /// and other programs' entries keep whether they are on it, and go back to where unplaced
    /// entries appear — which is where they were before anyone arranged anything.
    /// </summary>
    [RelayCommand]
    private void ResetMenu()
    {
        var commands = Commands.Where(c => c.IsInMenu).ToList();
        var shell = MenuRows.Where(r => r.Kind == MenuRowKind.Shell).ToList();

        MenuRows.Clear();
        foreach (var token in MenuLayoutRules.Default)
        {
            if (MenuLayoutRules.IsSeparator(token)) MenuRows.Add(MenuRowViewModel.Separator());
            else if (MenuLayoutRules.IsBuiltIn(token, out var id) && BuiltInMenuItems.Find(id) is { } item)
                MenuRows.Add(MenuRowViewModel.ForBuiltIn(item));
            else if (token == MenuLayoutRules.More) MenuRows.Add(MenuRowViewModel.ForMore());
        }

        PlaceUnplaced(commands.Select(MenuRowViewModel.ForCommand));
        PlaceUnplaced(shell);
        SelectedMenuRow = null;
    }

    /// <summary>A new command, straight onto the menu under the selected row.</summary>
    [RelayCommand]
    private void NewCustomCommand()
    {
        var item = new CustomCommandItemViewModel { Name = "New command" };
        Commands.Add(item);
        var row = MenuRowViewModel.ForCommand(item);
        MenuRows.Insert(InsertionIndex(), row);
        SelectedMenuRow = row;
        SelectedCommand = item;
    }

    /// <summary>Deletes the selected command outright — off the menu and out of settings.</summary>
    [RelayCommand]
    private void DeleteCustomCommand()
    {
        if (SelectedCommand is not { } selected) return;

        if (MenuRows.FirstOrDefault(r => r.Command == selected) is { } row) MenuRows.Remove(row);

        var index = Commands.IndexOf(selected);
        Commands.Remove(selected);
        SelectedCommand = Commands.Count > 0 ? Commands[Math.Min(index, Commands.Count - 1)] : null;
    }

    /// <summary>The preview row an entry from the right-hand column has, or would have.</summary>
    private MenuRowViewModel? RowFor(object choice)
    {
        var token = choice switch
        {
            MenuChoiceViewModel c => c.Token,
            CustomCommandItemViewModel c => MenuLayoutRules.CommandToken(c.Id),
            _ => null,
        };
        if (token is null) return null;

        if (MenuRows.FirstOrDefault(r => r.Token.Equals(token, StringComparison.OrdinalIgnoreCase)) is { } row)
            return row;

        return choice switch
        {
            CustomCommandItemViewModel command => MenuRowViewModel.ForCommand(command),
            MenuChoiceViewModel c when MenuLayoutRules.IsBuiltIn(c.Token, out var id) &&
                                       BuiltInMenuItems.Find(id) is { } item => MenuRowViewModel.ForBuiltIn(item),
            MenuChoiceViewModel c => MenuRowViewModel.ForShell(c.Token, c.Name, c.Detail),
            _ => null,
        };
    }

    /// <summary>
    /// Writes the menu: the order, and whether each entry is on it. Ids hidden earlier but not
    /// offered today stay hidden — the extension scan may still be running, or the extension may be
    /// uninstalled for now, or the id may be another version's (see
    /// <see cref="ShellMenuRules.HiddenAfterSave"/>). A command's membership travels with the
    /// command itself.
    /// </summary>
    private void ApplyMenuLayout()
    {
        _settings.ContextMenuLayout = MenuRows.Select(r => r.Token).ToList();
        _settings.HiddenShellExtensions = ShellMenuRules.HiddenAfterSave(
            _settings.HiddenShellExtensions,
            ShellChoices.Select(c => (c.Id, c.IsInMenu))).ToList();
        _settings.HiddenBuiltInMenuItems = ShellMenuRules.HiddenAfterSave(
            _settings.HiddenBuiltInMenuItems,
            BuiltInChoices.Select(c => (c.Id, c.IsInMenu))).ToList();
    }
}
