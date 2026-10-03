using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.UndoHistory;
using BertBrowser.App.Services.Commands;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// The undo history as the History window and the title-bar menus show it: newest first, with a
/// divider where the cursor is.
/// </summary>
/// <remarks>
/// <para>
/// Always present, like the transfer queue, and rebuilt from the stack after every change rather
/// than patched — the list is at most a couple of hundred rows, and a rebuild cannot drift from the
/// stack it describes. Which rows were expanded is carried across by entry id.
/// </para>
/// <para>
/// It runs nothing itself: the row commands call back into the shell, which owns the busy flag every
/// write shares.
/// </para>
/// </remarks>
public sealed partial class UndoHistoryViewModel : ObservableObject
{
    private readonly Func<UndoEntry, Task> _undoTo;
    private readonly Func<UndoEntry, Task> _redoTo;
    private readonly HashSet<int> _expanded = [];

    public UndoHistoryViewModel(Func<UndoEntry, Task> undoTo, Func<UndoEntry, Task> redoTo)
    {
        _undoTo = undoTo;
        _redoTo = redoTo;
    }

    /// <summary><see cref="UndoEntryViewModel"/>s newest first, with one <see cref="UndoCursorMarker"/>
    /// between the undone ones above it and the done ones below — the order an editor's history
    /// panel reads in, where the top is what happened last.</summary>
    public ObservableCollection<object> Rows { get; } = [];

    /// <summary>Done entries, next undo first — what the Undo dropdown lists.</summary>
    public IReadOnlyList<UndoEntryViewModel> Undoable { get; private set; } = [];

    /// <summary>Undone entries, next redo first — what the Redo dropdown lists.</summary>
    public IReadOnlyList<UndoEntryViewModel> Redoable { get; private set; } = [];

    [ObservableProperty]
    private bool _hasEntries;

    /// <summary>"Holding 3.2 GB of 10 GB · 7 of 50 actions".</summary>
    [ObservableProperty]
    private string _footerText = "";

    /// <summary>"12 older actions released (4.1 GB committed)", or empty.</summary>
    [ObservableProperty]
    private string _releasedText = "";

    /// <summary>What "Clear history" would commit, for its confirmation.</summary>
    public HeldSize Held { get; private set; }

    /// <summary>False while any write is running, so no row offers a step that would be refused.</summary>
    [ObservableProperty]
    private bool _canStep = true;

    public void Refresh(UndoStack stack, DateTime nowUtc, bool canStep)
    {
        CanStep = canStep;
        Held = stack.Held;

        var rows = new List<UndoEntryViewModel>();
        for (var i = stack.Entries.Count - 1; i >= 0; i--)
        {
            var entry = stack.Entries[i];
            rows.Add(new UndoEntryViewModel(this, entry, nowUtc,
                isNextUndo: ReferenceEquals(entry, stack.NextUndo),
                isNextRedo: ReferenceEquals(entry, stack.NextRedo))
            {
                IsExpanded = _expanded.Contains(entry.Id),
            });
        }

        Undoable = [.. rows.Where(r => r.IsDone)];
        Redoable = [.. rows.Where(r => !r.IsDone).Reverse()];

        Rows.Clear();
        foreach (var row in rows.Where(r => !r.IsDone)) Rows.Add(row);
        if (Redoable.Count > 0) Rows.Add(new UndoCursorMarker());
        foreach (var row in Undoable) Rows.Add(row);

        HasEntries = rows.Count > 0;

        var held = stack.Held;
        var limit = Gigabytes(stack.Budget.MaxHeldBytes);
        var holding = held.Bytes == 0 && held.Complete
            ? $"Holding no set-aside files (limit {limit})"
            : $"Holding {(held.Complete ? "" : "at least ")}{ByteSizeFormatter.Format(held.Bytes)} of {limit}";
        FooterText = $"{holding} · {stack.Entries.Count:N0} of {stack.Budget.MaxEntries:N0} actions";
        ReleasedText = stack.ReleasedCount == 0
            ? ""
            : $"{UndoText.Items(stack.ReleasedCount).Replace("item", "older action")} released to stay within the limits" +
              (stack.ReleasedBytes > 0 ? $" ({ByteSizeFormatter.Format(stack.ReleasedBytes)} removed for good)" : "");

        _expanded.IntersectWith(stack.Entries.Select(e => e.Id));
    }

    /// <summary>A limit as the Settings page offers it — whole gigabytes — rather than as a byte
    /// count formatted to three decimals.</summary>
    private static string Gigabytes(long bytes) =>
        bytes % (1L << 30) == 0 ? $"{bytes >> 30:N0} GB" : ByteSizeFormatter.Format(bytes);

    internal void SetExpanded(int id, bool expanded)
    {
        if (expanded) _expanded.Add(id);
        else _expanded.Remove(id);
    }

    internal Task UndoTo(UndoEntry entry) => _undoTo(entry);

    internal Task RedoTo(UndoEntry entry) => _redoTo(entry);
}

/// <summary>One operation in the history list.</summary>
public sealed partial class UndoEntryViewModel : ObservableObject
{
    /// <summary>Beyond this, a row's detail says how many more there are instead of listing them —
    /// a merge can be five thousand items, and nobody reads the five-thousandth.</summary>
    public const int DetailCap = 200;

    private readonly UndoHistoryViewModel _owner;

    internal UndoEntryViewModel(
        UndoHistoryViewModel owner, UndoEntry entry, DateTime nowUtc, bool isNextUndo, bool isNextRedo)
    {
        _owner = owner;
        Entry = entry;
        IsNextUndo = isNextUndo;
        IsNextRedo = isNextRedo;

        var record = entry.Record;
        Description = record.Description;
        IconKey = IconFor(record.Kind);
        IsDone = entry.IsDone;
        WhenText = RelativeTime.Format(entry.ChangedUtc, nowUtc);
        ItemsText = UndoText.Items(record.ItemCount);
        HeldText = entry.Held.Bytes > 0 || !entry.Held.Complete
            ? "· " + (entry.Held.Complete ? "" : "≥ ") + ByteSizeFormatter.Format(entry.Held.Bytes) + " held"
            : "";
        Badge = BadgeFor(entry);
        LastProblem = entry.LastReport is { Clean: false } report
            ? report.Failures[0] + (report.Failures.Count > 1 ? $" (and {report.Failures.Count - 1:N0} more)" : "")
            : "";
    }

    public UndoEntry Entry { get; }

    public int Id => Entry.Id;

    public string Description { get; }

    /// <summary>The <c>Icon.*</c> resource for this kind of operation.</summary>
    public string IconKey { get; }

    public bool IsDone { get; }

    public bool IsNextUndo { get; }

    public bool IsNextRedo { get; }

    public string WhenText { get; }

    public string ItemsText { get; }

    public string HeldText { get; }

    /// <summary>"Undone", "Partly undone", "Undo failed"… or empty for an entry simply in effect.</summary>
    public string Badge { get; }

    public bool HasBadge => Badge.Length > 0;

    /// <summary>Why the last step was not clean, or empty.</summary>
    public string LastProblem { get; }

    public bool HasProblem => LastProblem.Length > 0;

    /// <summary>The outline for <see cref="IconKey"/>, or null outside a running app — the same
    /// lookup the Settings sidebar makes, for the same reason: the kind is only known here.</summary>
    public Geometry? Icon => Application.Current?.TryFindResource(IconKey) as Geometry;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ExpandText))]
    private bool _isExpanded;

    public string ExpandText => IsExpanded ? "Hide items" : "Items";

    partial void OnIsExpandedChanged(bool value) => _owner.SetExpanded(Id, value);

    /// <summary>Built only when a row is opened, and capped.</summary>
    public IReadOnlyList<string> Details
    {
        get
        {
            var details = Entry.Record.Details;
            var lines = details.Take(DetailCap).Select(d =>
                string.Equals(d.From, d.To, StringComparison.OrdinalIgnoreCase)
                    ? d.From
                    : $"{d.From}  →  {d.To}").ToList();
            if (details.Count > DetailCap) lines.Add($"…and {details.Count - DetailCap:N0} more");
            return lines;
        }
    }

    /// <summary>Where the operation's result can be seen now: the first item's destination while
    /// it is in effect, its origin once it has been undone.</summary>
    public string? RevealPath =>
        Entry.Record.Details.FirstOrDefault() is { } first
            ? (IsDone && Entry.Record.Kind is not (UndoKind.Delete) ? first.To : first.From)
            : null;

    public bool CanUndoToHere => IsDone && _owner.CanStep;

    public bool CanRedoToHere => !IsDone && _owner.CanStep;

    [RelayCommand(CanExecute = nameof(CanUndoToHere))]
    private Task UndoToHere() => _owner.UndoTo(Entry);

    [RelayCommand(CanExecute = nameof(CanRedoToHere))]
    private Task RedoToHere() => _owner.RedoTo(Entry);

    [RelayCommand]
    private void ToggleExpanded() => IsExpanded = !IsExpanded;

    public static string IconFor(UndoKind kind) => kind switch
    {
        UndoKind.Rename => "Icon.Rename",
        UndoKind.Move => "Icon.MoveToFolder",
        UndoKind.Paste => "Icon.Paste",
        UndoKind.Delete => "Icon.Delete",
        UndoKind.ArchiveEdit => "Icon.Archive",
        UndoKind.Sync => "Icon.Compare",
        _ => "Icon.Undo",
    };

    private static string BadgeFor(UndoEntry entry)
    {
        if (entry.LastReport is not { } report || entry.LastDirection is not { } direction)
            return "";

        var undo = direction == UndoDirection.Undo;
        return report.Verdict switch
        {
            StepVerdict.Failed => undo ? "Undo failed" : "Redo failed",
            StepVerdict.Partial => undo ? "Partly undone" : "Partly redone",
            _ => entry.IsDone ? "" : "Undone",
        };
    }
}

/// <summary>The divider between what has been undone and what is still in effect. Shown only when
/// something has been undone — with nothing above it, it would divide nothing.</summary>
public sealed record UndoCursorMarker
{
    public string Text =>
        $"Undone above — {GestureText.OrName("edit.redo")} redoes · In effect below — {GestureText.OrName("edit.undo")} undoes";
}
