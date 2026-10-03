using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.ViewModels;

/// <summary>Something the palette can run: what it lists, and what choosing it does.</summary>
/// <param name="Bindable">Whether a keyboard shortcut can be given to it — a catalogue command
/// or one of the user's own, but not a bookmark or a workspace, whose names can change.</param>
/// <param name="Remembered">Whether running it goes into "Recent". False for things that are a
/// position rather than an identity: the third open tab is not the same tab tomorrow.</param>
/// <param name="Completion">For a place: the text that carries on from it, so Tab can descend
/// into a folder instead of going there.</param>
public sealed record PaletteItem(
    PaletteEntry Entry, Action Run, bool Bindable = false, bool Remembered = true, string? Completion = null);

/// <summary>One line of the palette as the view draws it.</summary>
public sealed class PaletteRowViewModel
{
    public PaletteRowViewModel(PaletteRow row, PaletteItem? item, bool pinned, bool showCategory)
    {
        Row = row;
        Item = item;
        IsPinned = pinned;

        var entry = row.Entry;
        // What sits to the right of the name: why it cannot run, when it cannot; otherwise where
        // it is or what it belongs to. A reason outranks the rest because it is the answer to the
        // question the dimmed row raises.
        Detail = row.Kind switch
        {
            PaletteRowKind.Category => row.Count == 1 ? "1 command" : $"{row.Count:N0} commands",
            PaletteRowKind.Entry when entry is { IsAvailable: false } => entry.Unavailable!,
            PaletteRowKind.Entry when entry is { Detail.Length: > 0 } => entry.Detail,
            PaletteRowKind.Entry when showCategory => entry!.Category,
            _ => "",
        };
    }

    public PaletteRow Row { get; }

    public PaletteItem? Item { get; }

    public string Text => Row.Text;

    public IReadOnlyList<MatchRange> Highlights => Row.Highlights ?? [];

    public string Detail { get; }

    public string Gesture => Row.Entry?.Gesture ?? "";

    public bool IsHeading => Row.Kind == PaletteRowKind.Heading;

    public bool IsCategory => Row.Kind == PaletteRowKind.Category;

    public bool IsEntry => Row.Kind == PaletteRowKind.Entry;

    public bool IsAvailable => Row.Entry?.IsAvailable ?? true;

    public bool IsPinned { get; }

    /// <summary>Whether the row's Pin button means anything: an entry, and one worth keeping.</summary>
    public bool CanPin => IsEntry && Item is { Remembered: true };

    public bool CanBind => IsEntry && Item is { Bindable: true };

    public string PinToolTip => IsPinned ? "Unpin (Alt+P)" : "Pin to the top of the palette (Alt+P)";

    public Geometry? Icon =>
        Row.Entry?.Icon is { } key ? Application.Current?.TryFindResource(key) as Geometry : null;

    public Geometry? PinIcon =>
        Application.Current?.TryFindResource(IsPinned ? "Icon.PinOff" : "Icon.Pin") as Geometry;
}

/// <summary>
/// The command palette: a box, and under it whatever <see cref="PaletteRules"/> says belongs
/// there for what has been typed. This class holds no rule of its own about what to list or in
/// what order — it gathers what can be run, asks, and draws the answer.
/// </summary>
public sealed partial class CommandPaletteViewModel : ObservableObject
{
    private readonly PaletteMemory _memory;

    private IReadOnlyList<PaletteItem> _items = [];
    private Dictionary<string, PaletteItem> _byId = new(StringComparer.Ordinal);
    private IReadOnlyList<string> _suggested = [];
    private Func<string, IReadOnlyList<PaletteItem>?> _places = _ => null;

    public CommandPaletteViewModel(PaletteMemory memory) => _memory = memory;

    public ObservableCollection<PaletteRowViewModel> Rows { get; } = [];

    [ObservableProperty]
    private string _query = "";

    [ObservableProperty]
    private PaletteRowViewModel? _selectedRow;

    /// <summary>What the strip under the list says: the selected row's reason when it cannot
    /// run, otherwise the keys that work here.</summary>
    [ObservableProperty]
    private string _hint = "";

    [ObservableProperty]
    private bool _isEmpty;

    /// <summary>Asks the view to close, and then — once focus is back where it was — to run this.</summary>
    public event Action<Action?>? CloseRequested;

    /// <summary>Asks for the Keyboard settings page, on this command.</summary>
    public event Action<string>? ShortcutRequested;

    /// <summary>Asks the view to put the caret at the end of the box after its text was set.</summary>
    public event Action? CaretToEndRequested;

    /// <param name="items">Everything that can be run right now, catalogue order first.</param>
    /// <param name="suggested">What to offer unasked, best first.</param>
    /// <param name="places">Turns typed text into places to go, or null when it is not a path.</param>
    public void Open(
        IReadOnlyList<PaletteItem> items, IReadOnlyList<string> suggested,
        Func<string, IReadOnlyList<PaletteItem>?> places, string seed = "")
    {
        _items = items;
        _byId = new Dictionary<string, PaletteItem>(StringComparer.Ordinal);
        foreach (var item in items) _byId.TryAdd(item.Entry.Id, item);
        _suggested = suggested;
        _places = places;

        // The setter does nothing when the text has not changed, and re-opening on the same text
        // still has to rebuild: what can run has moved on since the palette was last up.
        if (Query == seed) Rebuild();
        else Query = seed;
    }

    partial void OnQueryChanged(string value) => Rebuild();

    partial void OnSelectedRowChanged(PaletteRowViewModel? value) => UpdateHint();

    private void Rebuild(string? keepSelected = null)
    {
        Rows.Clear();

        // A path is a destination, not a search: the rows are the place and what is under it.
        if (_places(Query) is { } places)
        {
            Rows.Add(new PaletteRowViewModel(new PaletteRow(PaletteRowKind.Heading, "Go to"), null, false, false));
            foreach (var place in places)
                Rows.Add(new PaletteRowViewModel(new PaletteRow(PaletteRowKind.Entry, place.Entry.Name, place.Entry), place, false, false));
        }
        else
        {
            var state = new PaletteState(_memory.Pins, _memory.Recents, _suggested);
            var entries = _items.Select(i => i.Entry).ToList();
            var (category, _) = PaletteRules.SplitCategory(Query.Trim(), PaletteRules.Categories(entries));

            foreach (var row in PaletteRules.Build(Query, entries, state))
            {
                var item = row.Entry is { } entry ? _byId.GetValueOrDefault(entry.Id) : null;
                Rows.Add(new PaletteRowViewModel(
                    row, item,
                    pinned: row.Entry is { } e && _memory.IsPinned(e.Id),
                    // Inside one category every row would say the same thing.
                    showCategory: category is null));
            }
        }

        IsEmpty = !Rows.Any(r => !r.IsHeading);
        // Enter should do something: start on the first row that can run. With something typed
        // that is the best match unless it is unavailable, in which case the best match stays
        // selected so its reason is what the hint strip shows.
        SelectedRow =
            (keepSelected is null ? null : Rows.FirstOrDefault(r => r.Row.Entry?.Id == keepSelected))
            ?? (Query.Trim().Length == 0 ? Rows.FirstOrDefault(r => !r.IsHeading && r.IsAvailable) : null)
            ?? Rows.FirstOrDefault(r => !r.IsHeading);
        UpdateHint();
    }

    /// <summary>Moves the selection by <paramref name="delta"/> choosable rows, stepping over
    /// headings and wrapping at either end.</summary>
    public void Move(int delta)
    {
        var choosable = Rows.Where(r => !r.IsHeading).ToList();
        if (choosable.Count == 0) return;

        var at = SelectedRow is null ? -1 : choosable.IndexOf(SelectedRow);
        var next = at < 0
            ? (delta > 0 ? 0 : choosable.Count - 1)
            : ((at + delta) % choosable.Count + choosable.Count) % choosable.Count;
        SelectedRow = choosable[next];
    }

    /// <summary>Moves to the first or last choosable row, without wrapping — what paging a long
    /// list by more than it has left should do.</summary>
    public void MoveToEdge(bool last)
    {
        var choosable = Rows.Where(r => !r.IsHeading).ToList();
        if (choosable.Count > 0) SelectedRow = last ? choosable[^1] : choosable[0];
    }

    /// <summary>Enter: runs the selected entry, or narrows to the selected category.</summary>
    public void Activate()
    {
        if (SelectedRow is not { } row) return;

        if (row.IsCategory)
        {
            Type(row.Row.Token + " ");
            return;
        }

        if (row.Item is not { } item) return;

        // Left open, with the reason in the hint strip: closing on a command that then does
        // nothing would look exactly like the command being broken.
        if (!row.IsAvailable) return;

        if (item.Remembered) _memory.NoteRun(item.Entry.Id);
        CloseRequested?.Invoke(item.Run);
    }

    /// <summary>Tab: carries on typing from the selected row — into a category, or down into a
    /// folder — instead of choosing it.</summary>
    public void Complete()
    {
        if (SelectedRow is not { } row) return;

        if (row.IsCategory) Type(row.Row.Token + " ");
        else if (row.Item?.Completion is { } completion) Type(completion);
    }

    public void TogglePin() => TogglePin(SelectedRow);

    public void TogglePin(PaletteRowViewModel? row)
    {
        if (row is not { CanPin: true, Row.Entry: { } entry }) return;

        _memory.TogglePin(entry.Id);
        Rebuild(keepSelected: entry.Id);
    }

    public void ChangeShortcut() => ChangeShortcut(SelectedRow);

    public void ChangeShortcut(PaletteRowViewModel? row)
    {
        if (row is not { CanBind: true, Row.Entry: { } entry }) return;
        CloseRequested?.Invoke(() => ShortcutRequested?.Invoke(entry.Id));
    }

    public void Dismiss() => CloseRequested?.Invoke(null);

    private void Type(string text)
    {
        Query = text;
        CaretToEndRequested?.Invoke();
    }

    private void UpdateHint()
    {
        Hint = SelectedRow switch
        {
            null => "Nothing matches. Try another word for it, or clear the box to browse by category.",
            { IsCategory: true } => "Enter or Tab lists this category",
            { IsAvailable: false } row => $"Unavailable — {row.Detail}",
            { Item.Completion: not null } => "Enter goes there · Tab carries on into it · Esc closes",
            { CanBind: true } => "Enter runs · Alt+P pins · Alt+K changes its shortcut · Esc closes",
            { CanPin: true } => "Enter runs · Alt+P pins · Esc closes",
            _ => "Enter runs · Esc closes",
        };
    }
}
