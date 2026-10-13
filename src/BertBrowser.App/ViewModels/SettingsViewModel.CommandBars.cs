using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.CommandBars;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.ViewModels;

/// <summary>One bar that can be edited: the menu bar, the title bar's toolbar, or a row.</summary>
public sealed record BarTargetViewModel(BarTarget Target, string Name, string IconKey)
{
    public bool IsToolbar => Target.Kind != BarTargetKind.MenuBar;

    public bool IsRow => Target.Kind == BarTargetKind.Row;

    public Geometry? Icon => Application.Current?.TryFindResource(IconKey) as Geometry;
}

/// <summary>One item of the bar being edited, as the middle list shows it.</summary>
public sealed class BarItemRowViewModel(BarItem item, string name, string detail, string gesture, string? iconKey)
{
    public BarItem Item { get; } = item;

    public string Name { get; } = name;

    /// <summary>The page's own note: "submenu · 6 items", "list", "not in this version".</summary>
    public string Detail { get; } = detail;

    public string Gesture { get; } = gesture;

    public bool IsSeparator => Item.IsSeparator;

    public bool IsMenu => Item.IsMenu;

    public Geometry? Icon => iconKey is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;
}

/// <summary>One line of the picker: a category heading, or something that can be added.</summary>
public sealed class BarChoiceViewModel
{
    private BarChoiceViewModel(string name) => Name = name;

    public static BarChoiceViewModel Heading(string category) => new(category) { IsHeading = true };

    public static BarChoiceViewModel For(BarItem item, string name, string? iconKey, int placed) =>
        new(name) { Item = item, IconKey = iconKey, Placed = placed };

    public string Name { get; }

    public bool IsHeading { get; private init; }

    /// <summary>What adding this choice puts on the bar. Null for a heading.</summary>
    public BarItem? Item { get; private init; }

    public string? IconKey { get; private init; }

    /// <summary>How many times it is on a menu or toolbar already. A command may be placed more
    /// than once — on a menu and on a toolbar — so this is a count, not a tick.</summary>
    public int Placed { get; private init; }

    public string PlacedText => Placed switch { 0 => "", 1 => "placed", _ => $"placed ×{Placed}" };

    public Geometry? Icon => IconKey is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;
}

/// <summary>One saved arrangement, as the page's list shows it.</summary>
public sealed record SavedBarsItemViewModel(string Name, string ShapeText, bool IsCurrent);

/// <summary>
/// The Menus and toolbars page: the arrangement in force, edited in place, and the arrangements
/// saved under a name.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing here goes through <see cref="Apply"/>.</b> Every edit is written at once by
/// <see cref="CommandBarService"/>, the arrangement's only writer, for the reason the Keyboard page
/// gives: it can be replaced from a menu or the palette while this page is open, and a page that
/// wrote its own copy on a debounce would put the old one back. The window redraws its bars on
/// the service's event, so the real menu bar above the page is the preview.
/// </para>
/// <para>
/// <b>The editor drills down rather than showing a tree.</b> The middle list is always the items
/// of one place — a bar, or a submenu somewhere inside it — so reordering is the same flat drag
/// every other list on these pages uses, and a submenu is opened to be edited. What an edit may
/// do is <see cref="CommandBarEdit"/>'s to say; this only aims it.
/// </para>
/// </remarks>
public sealed partial class SettingsViewModel
{
    private readonly CommandBarService? _bars;

    /// <summary>Indices down through submenus from <see cref="SelectedBar"/> to the place on show.</summary>
    private readonly List<int> _barPath = [];

    private bool _loadingBars;

    public bool CanEditBars => _bars is not null;

    /// <summary>Asked before an edit that would discard an arrangement kept nowhere else — Reset,
    /// or recalling a saved one. Set by the window, which owns the question; absent, the answer
    /// is yes.</summary>
    public Func<string, bool>? ConfirmDiscardBars { get; set; }

    public ObservableCollection<BarTargetViewModel> BarTargets { get; } = [];

    public ObservableCollection<BarItemRowViewModel> BarRows { get; } = [];

    public ObservableCollection<BarChoiceViewModel> BarChoices { get; } = [];

    public ObservableCollection<SavedBarsItemViewModel> SavedBars { get; } = [];

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RemoveToolbarRowCommand))]
    private BarTargetViewModel? _selectedBar;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RemoveBarRowCommand), nameof(OpenBarRowCommand))]
    private BarItemRowViewModel? _selectedBarRow;

    [ObservableProperty]
    private SavedBarsItemViewModel? _selectedSavedBars;

    /// <summary>Narrows the picker: part of a command's name, or its category.</summary>
    [ObservableProperty]
    private string _barFilter = "";

    /// <summary>Shows only what is on no menu and no toolbar — where a command added by a later
    /// version turns up for somebody whose arrangement is their own.</summary>
    [ObservableProperty]
    private bool _barUnplacedOnly;

    /// <summary>"Menu bar › File › New": where the middle list is.</summary>
    [ObservableProperty]
    private string _barLocation = "";

    /// <summary>What the arrangement in force is: the default, a saved one by name, or its own.</summary>
    [ObservableProperty]
    private string _barsStatus = "";

    /// <summary>The words for the selected item, in the box under the list. Committed by
    /// <see cref="CommitBarCaption"/>, not as it is typed: each commit redraws the real menu bar.</summary>
    [ObservableProperty]
    private string _barCaption = "";

    /// <summary>The hint under the caption box, which says what an empty one means.</summary>
    [ObservableProperty]
    private string _barCaptionHint = "";

    public bool HasSavedBars => SavedBars.Count > 0;

    public bool CanGoUpBars => _barPath.Count > 0;

    /// <summary>The menu bar's own level holds menus only, so the picker has nothing to offer it.</summary>
    public bool CanAddBarChoices => SelectedBar is { } bar && (bar.IsToolbar || _barPath.Count > 0);

    public bool CanAddBarSeparator => CanAddBarChoices;

    public bool IsEditingToolbar => SelectedBar is { IsToolbar: true } && _barPath.Count == 0;

    public bool CanEditBarCaption => SelectedBarRow is { IsSeparator: false, Item.IsList: false };

    public string AddSubmenuLabel => SelectedBar is { IsToolbar: false } && _barPath.Count == 0 ? "Menu" : "Submenu";

    /// <summary>Whether the toolbar on show writes its buttons' names beside their icons.</summary>
    public bool BarShowsText
    {
        get => _bars is not null && SelectedBar is { } bar && CommandBarEdit.ShowsText(_bars.Current, bar.Target);
        set
        {
            if (_bars is null || SelectedBar is not { } bar) return;
            Commit(CommandBarEdit.WithShowText(_bars.Current, bar.Target, value));
        }
    }

    /// <summary>Whether the selected toolbar button writes its name whatever the toolbar does.</summary>
    public bool BarRowShowsText
    {
        get => SelectedBarRow?.Item.ShowText == true;
        set
        {
            if (SelectedBarRow is not { } row) return;

            var index = BarRows.IndexOf(row);
            if (index >= 0) ReplaceRow(index, row.Item with { ShowText = value ? true : null });
        }
    }

    partial void OnSelectedBarChanged(BarTargetViewModel? value)
    {
        if (_loadingBars) return;

        _barPath.Clear();
        LoadBarRows(select: 0);
    }

    partial void OnSelectedBarRowChanged(BarItemRowViewModel? value)
    {
        BarCaption = value is { IsSeparator: false, Item.IsList: false }
            ? value.Item.Caption ?? ""
            : "";
        BarCaptionHint = value switch
        {
            { IsMenu: true } => "A menu's name. Put & before the letter Alt should pick it by.",
            { Item.IsCommand: true } => $"Leave empty to call it \"{CommandBarCaptions.Describe(value.Item with { Caption = null })}\".",
            _ => "",
        };
        OnPropertyChanged(nameof(CanEditBarCaption));
        OnPropertyChanged(nameof(BarRowShowsText));
    }

    partial void OnBarFilterChanged(string value) => LoadBarChoices();

    partial void OnBarUnplacedOnlyChanged(bool value) => LoadBarChoices();

    // --- Loading ---

    /// <summary>
    /// Re-reads everything from the arrangement in force, keeping the place on show if it still
    /// exists. Called after each of this page's own edits and by the window when the arrangement
    /// changes from somewhere else.
    /// </summary>
    public void LoadBars()
    {
        if (_bars is null) return;

        var set = _bars.Current;
        var selected = SelectedBar?.Target ?? BarTarget.MenuBar;
        var selectedRow = SelectedBarRow is { } row ? BarRows.IndexOf(row) : 0;

        _loadingBars = true;
        try
        {
            BarTargets.Clear();
            BarTargets.Add(new BarTargetViewModel(BarTarget.MenuBar, CommandBarEdit.NameOf(BarTarget.MenuBar), "Icon.DetailsList"));
            BarTargets.Add(new BarTargetViewModel(BarTarget.TitleBar, CommandBarEdit.NameOf(BarTarget.TitleBar), "Icon.Reorder"));
            for (var i = 0; i < set.Rows.Count; i++)
                BarTargets.Add(new BarTargetViewModel(BarTarget.ForRow(i), CommandBarEdit.NameOf(BarTarget.ForRow(i)), "Icon.Reorder"));

            SelectedBar = BarTargets.FirstOrDefault(t => t.Target == selected) ?? BarTargets[0];
        }
        finally
        {
            _loadingBars = false;
        }

        // The place may have gone: a row removed, a submenu deleted from a menu elsewhere.
        while (_barPath.Count > 0 && !CommandBarEdit.Exists(set, SelectedBar!.Target, _barPath))
            _barPath.RemoveAt(_barPath.Count - 1);

        LoadBarRows(selectedRow);
        LoadSavedBars();
    }

    private void LoadBarRows(int select)
    {
        if (_bars is null || SelectedBar is not { } bar) return;

        var set = _bars.Current;
        var items = CommandBarEdit.ItemsAt(set, bar.Target, _barPath);

        BarRows.Clear();
        foreach (var item in items) BarRows.Add(RowFor(item));
        SelectedBarRow = BarRows.Count == 0 ? null : BarRows[Math.Clamp(select, 0, BarRows.Count - 1)];

        BarLocation = string.Join("  ›  ", CommandBarEdit.Trail(set, bar.Target, _barPath));
        BarsStatus = _bars.IsDefault ? "The arrangement BertBrowser ships with"
            : _bars.CurrentName is { } name ? $"Saved as \"{name}\""
            : "Your own arrangement, not saved under a name";

        OnPropertyChanged(nameof(CanGoUpBars));
        OnPropertyChanged(nameof(CanAddBarChoices));
        OnPropertyChanged(nameof(CanAddBarSeparator));
        OnPropertyChanged(nameof(IsEditingToolbar));
        OnPropertyChanged(nameof(AddSubmenuLabel));
        OnPropertyChanged(nameof(BarShowsText));
        BarUpCommand.NotifyCanExecuteChanged();
        AddBarSeparatorCommand.NotifyCanExecuteChanged();
        ResetBarsCommand.NotifyCanExecuteChanged();

        LoadBarChoices();
    }

    private BarItemRowViewModel RowFor(BarItem item)
    {
        var name = CommandBarCaptions.Describe(item);

        if (item.IsMenu)
        {
            var count = item.Items?.Count(i => !i.IsSeparator) ?? 0;
            return new BarItemRowViewModel(item, name, $"submenu · {count} item{(count == 1 ? "" : "s")}", "", "Icon.ChevronDown");
        }

        if (item.IsList)
        {
            var list = CommandBarLists.Find(item.Id);
            return new BarItemRowViewModel(item, name, list is null ? "not in this version" : "list", "", list?.Icon ?? "Icon.Warning");
        }

        if (item.IsCommand && item.Id is { } id)
        {
            var (known, icon) = DescribeBarCommand(id);
            var named = item.Caption is { Length: > 0 } ? name : known ?? name;
            return new BarItemRowViewModel(
                item, named, known is null ? "not in this version" : item.ShowText == true ? "with its name" : "",
                _keymap?.GestureText(id) ?? "", icon ?? "Icon.Warning");
        }

        return new BarItemRowViewModel(item, name, item.IsSeparator ? "" : "not in this version", "", null);
    }

    /// <summary>A command's menu name and icon, or nulls for an id nothing here answers to.</summary>
    private (string? Name, string? Icon) DescribeBarCommand(string id)
    {
        if (CommandCatalog.Find(id) is { } command)
            return (CommandBarCaptions.For(id) ?? command.Name, command.Icon);

        return _settings.CustomCommands.FirstOrDefault(c => KeymapService.CustomId(c.Id) == id) is { } custom
            ? (custom.Name, custom.RunElevated ? "Icon.Shield" : "Icon.CustomCommand")
            : (null, null);
    }

    private void LoadBarChoices()
    {
        if (_bars is null) return;

        var placed = CommandBarRules.Placements(_bars.Current);
        var filter = BarFilter.Trim();
        BarChoices.Clear();

        var commands = CommandCatalog.All
            .Select(c => (c.Id, Name: c.Name, c.Category, c.Icon, c.Aliases))
            .Concat(_settings.CustomCommands.DistinctBy(c => c.Id).Select(c => (
                Id: KeymapService.CustomId(c.Id), Name: c.Name, Category: "Custom commands",
                Icon: (string?)(c.RunElevated ? "Icon.Shield" : "Icon.CustomCommand"),
                Aliases: (IReadOnlyList<string>)[])));

        string? heading = null;
        void Add(string category, BarChoiceViewModel choice)
        {
            if (category != heading)
            {
                BarChoices.Add(BarChoiceViewModel.Heading(category));
                heading = category;
            }
            BarChoices.Add(choice);
        }

        foreach (var (id, name, category, icon, aliases) in commands)
        {
            var count = placed.GetValueOrDefault(id);
            if (BarUnplacedOnly && count > 0) continue;
            if (filter.Length > 0 && !CommandMatcher.Match(filter, name, aliases, category, "").IsMatch) continue;

            Add(category, BarChoiceViewModel.For(BarItem.Command(id), name, icon, count));
        }

        // Lists are not commands and cannot be "unplaced" in the sense that filter means.
        if (BarUnplacedOnly) return;

        foreach (var list in CommandBarLists.All)
        {
            if (filter.Length > 0 && !CommandMatcher.Match(filter, list.Name, [], "Lists", "").IsMatch) continue;

            var count = CommandBarRules.Flatten(_bars.Current).Count(i => i.IsList && i.Id == list.Id);
            Add("Lists", BarChoiceViewModel.For(BarItem.List(list.Id), list.Name, list.Icon, count));
        }
    }

    private void LoadSavedBars()
    {
        if (_bars is null) return;

        var current = _bars.CurrentName;
        var selected = SelectedSavedBars?.Name;

        SavedBars.Clear();
        foreach (var saved in _bars.Saved)
        {
            SavedBars.Add(new SavedBarsItemViewModel(
                saved.Name, SavedCommandBarRules.ShapeText(saved.Set),
                string.Equals(saved.Name, current, StringComparison.OrdinalIgnoreCase)));
        }
        SelectedSavedBars = SavedBars.FirstOrDefault(s => string.Equals(s.Name, selected, StringComparison.OrdinalIgnoreCase));
        OnPropertyChanged(nameof(HasSavedBars));
    }

    // --- Editing ---

    /// <summary>Puts an edited arrangement in force and re-reads the page from it, with the row
    /// at <paramref name="select"/> selected (the one already selected when null).</summary>
    private void Commit(CommandBarSet edited, int? select = null)
    {
        if (_bars is null) return;

        var keep = select ?? (SelectedBarRow is { } row ? BarRows.IndexOf(row) : 0);
        _bars.Set(edited);
        // The window hears the service and calls LoadBars; this is for a page with no window
        // behind it, and costs a second cheap read when there is one.
        LoadBars();
        if (BarRows.Count > 0) SelectedBarRow = BarRows[Math.Clamp(keep, 0, BarRows.Count - 1)];
    }

    private void ReplaceRow(int index, BarItem item)
    {
        if (_bars is null || SelectedBar is not { } bar) return;
        Commit(CommandBarEdit.Replace(_bars.Current, bar.Target, _barPath, index, item), index);
    }

    /// <summary>A row dragged within the list, to the index it should end up at — what
    /// <c>ListReorderDrag</c> reports.</summary>
    public void MoveBarRow(int from, int to)
    {
        if (_bars is null || SelectedBar is not { } bar || from < 0 || from >= BarRows.Count) return;

        to = Math.Clamp(to, 0, BarRows.Count - 1);
        if (to == from) return;
        Commit(CommandBarEdit.Move(_bars.Current, bar.Target, _barPath, from, to), to);
    }

    /// <summary>Alt+Up / Alt+Down on the selected row.</summary>
    public void NudgeSelectedBarRow(int delta)
    {
        if (_bars is null || SelectedBar is not { } bar || SelectedBarRow is not { } row) return;

        var from = BarRows.IndexOf(row);
        var to = from + delta;
        if (from < 0 || to < 0 || to >= BarRows.Count) return;
        Commit(CommandBarEdit.Move(_bars.Current, bar.Target, _barPath, from, to), to);
    }

    /// <summary>A choice dragged from the picker into a gap of the list.</summary>
    public void InsertBarChoice(object choice, int gap)
    {
        if (choice is BarChoiceViewModel { Item: { } item }) InsertBarItem(item, gap);
    }

    private void InsertBarItem(BarItem item, int? at = null)
    {
        if (_bars is null || SelectedBar is not { } bar) return;

        var index = at ?? (SelectedBarRow is { } row ? BarRows.IndexOf(row) + 1 : BarRows.Count);
        Commit(CommandBarEdit.Insert(_bars.Current, bar.Target, _barPath, index, item), index);
    }

    /// <summary>Adds a choice under the selected row, or at the end.</summary>
    [RelayCommand]
    private void AddBarChoice(BarChoiceViewModel? choice)
    {
        if (choice?.Item is { } item && CanAddBarChoices) InsertBarItem(item);
    }

    [RelayCommand(CanExecute = nameof(CanAddBarSeparator))]
    private void AddBarSeparator() => InsertBarItem(BarItem.Separator);

    /// <summary>A new, empty menu — a heading on the menu bar, a submenu inside one, a dropdown
    /// button on a toolbar — named so it can be seen and renamed.</summary>
    [RelayCommand]
    private void AddBarSubmenu() => InsertBarItem(BarItem.Menu("New menu"));

    [RelayCommand(CanExecute = nameof(HasSelectedBarRow))]
    private void RemoveBarRow(BarItemRowViewModel? row)
    {
        row ??= SelectedBarRow;
        if (_bars is null || SelectedBar is not { } bar || row is null) return;

        var index = BarRows.IndexOf(row);
        if (index < 0) return;
        Commit(CommandBarEdit.Remove(_bars.Current, bar.Target, _barPath, index), index);
    }

    private bool HasSelectedBarRow(BarItemRowViewModel? row) => (row ?? SelectedBarRow) is not null;

    /// <summary>Opens the selected submenu, so its items are the ones the list shows.</summary>
    [RelayCommand(CanExecute = nameof(CanOpenBarRow))]
    private void OpenBarRow(BarItemRowViewModel? row)
    {
        row ??= SelectedBarRow;
        if (row is not { IsMenu: true }) return;

        var index = BarRows.IndexOf(row);
        if (index < 0) return;

        _barPath.Add(index);
        LoadBarRows(select: 0);
    }

    private bool CanOpenBarRow(BarItemRowViewModel? row) => (row ?? SelectedBarRow) is { IsMenu: true };

    /// <summary>Back out of a submenu to the list it sits in, with it selected.</summary>
    [RelayCommand(CanExecute = nameof(CanGoUpBars))]
    private void BarUp()
    {
        if (_barPath.Count == 0) return;

        var cameFrom = _barPath[^1];
        _barPath.RemoveAt(_barPath.Count - 1);
        LoadBarRows(cameFrom);
    }

    /// <summary>Writes the caption box to the selected item. A menu must keep a name; a command
    /// with none goes back to its own.</summary>
    [RelayCommand]
    private void CommitBarCaption()
    {
        if (SelectedBarRow is not { } row || !CanEditBarCaption) return;

        var index = BarRows.IndexOf(row);
        var caption = BarCaption.Trim();
        if (row.IsMenu && caption.Length == 0)
        {
            BarCaption = row.Item.Caption ?? "";
            return;
        }

        var wanted = caption.Length == 0 ? null : caption;
        if (wanted == row.Item.Caption || index < 0) return;
        ReplaceRow(index, row.Item with { Caption = wanted });
    }

    [RelayCommand]
    private void AddToolbarRow()
    {
        if (_bars is null) return;

        var set = CommandBarEdit.AddRow(_bars.Current);
        _bars.Set(set);
        LoadBars();
        SelectedBar = BarTargets.LastOrDefault();
    }

    [RelayCommand(CanExecute = nameof(CanRemoveToolbarRow))]
    private void RemoveToolbarRow()
    {
        if (_bars is null || SelectedBar is not { IsRow: true } bar) return;

        _bars.Set(CommandBarEdit.RemoveRow(_bars.Current, bar.Target.Row));
        LoadBars();
    }

    private bool CanRemoveToolbarRow() => SelectedBar is { IsRow: true };

    [RelayCommand(CanExecute = nameof(CanResetBars))]
    private void ResetBars()
    {
        if (_bars is null) return;
        if (_bars.ReplacingLosesWork && ConfirmDiscardBars?.Invoke("the arrangement BertBrowser ships with") == false) return;

        _barPath.Clear();
        _bars.Reset();
        LoadBars();
    }

    private bool CanResetBars() => _bars is { IsDefault: false };

    /// <summary>Recalls a saved arrangement.</summary>
    [RelayCommand]
    private void ApplySavedBars(SavedBarsItemViewModel? saved)
    {
        saved ??= SelectedSavedBars;
        if (_bars is null || saved is null) return;
        if (_bars.ReplacingLosesWork && ConfirmDiscardBars?.Invoke($"\"{saved.Name}\"") == false) return;

        _barPath.Clear();
        _bars.Apply(saved.Name);
        LoadBars();
    }

    /// <summary>Forgets a saved arrangement. The one in force is untouched, whichever it was.</summary>
    public void RemoveSavedBars(SavedBarsItemViewModel saved)
    {
        _bars?.Remove(saved.Name);
        LoadBars();
    }
}
