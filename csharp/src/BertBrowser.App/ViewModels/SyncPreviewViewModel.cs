using System.Collections.ObjectModel;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Compare;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.ViewModels;

/// <summary>One row of the sync preview: what would happen to one path, and whether it will.</summary>
public sealed partial class SyncActionViewModel : ObservableObject
{
    public SyncAction Action { get; }

    [ObservableProperty]
    private bool _ticked;

    public SyncActionViewModel(SyncAction action)
    {
        Action = action;
        _ticked = action.Ticked;
    }

    public string RelativeDisplay => Action.RelativeDisplay;

    public string KindDisplay => Action.Kind switch
    {
        SyncActionKind.Copy => "Copy",
        SyncActionKind.Overwrite => "Replace",
        _ => "Delete",
    };

    /// <summary>Blank when nothing measured it, never "0 bytes" — a size nobody knows and a size of
    /// nothing are different answers.</summary>
    public string SizeDisplay => Action.Bytes is { } bytes ? ByteSizeFormatter.Format(bytes) : "";

    /// <summary>Why this row is here, in the same words the drag-and-drop conflict dialog uses for
    /// the same situation.</summary>
    public string Detail => Action.Verdict switch
    {
        CompareVerdict.LeftOnly => Action.IsDirectory ? "New folder" : "New file",
        CompareVerdict.LeftNewer => "The left side is newer",
        CompareVerdict.RightNewer => "The right side is newer",
        CompareVerdict.RightOnly => "Only on the right",
        _ => "The two differ",
    };
}

/// <summary>
/// What a sync is about to do, before it does any of it.
/// </summary>
/// <remarks>
/// The rows are blocked by kind with the removals last, so the destructive part of the run is
/// visibly its own thing rather than mixed into a list of copies. Everything about the defaults
/// leans the same way: deletions are off, and so is overwriting a file the right side updated more
/// recently.
/// </remarks>
public sealed partial class SyncPreviewViewModel : ObservableObject
{
    private readonly Func<bool, SyncPreview> _plan;
    private readonly Func<SyncPreview, Action<TransferProgressViewModel?>, Task>? _run;

    /// <param name="run">Carries out the sync, handing back the progress surface to show while it
    /// does. Optional so the harness can pose the dialog without one.</param>
    public SyncPreviewViewModel(
        Func<bool, SyncPreview> plan,
        string leftPath,
        string rightPath,
        Func<SyncPreview, Action<TransferProgressViewModel?>, Task>? run = null)
    {
        _plan = plan;
        _run = run;
        LeftPath = leftPath;
        RightPath = rightPath;
        Rebuild();
    }

    /// <summary>
    /// True while the sync is running, which is what turns the dialog from a question into a
    /// report: the list goes read-only and the buttons become a bar and a Cancel.
    /// </summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanRun))]
    private bool _isRunning;

    /// <summary>The live byte-level state of the run, or null when none is going. Bound inside the
    /// dialog rather than left to the status bar, which is behind a modal window and cannot be
    /// clicked even when it can be seen.</summary>
    [ObservableProperty]
    private TransferProgressViewModel? _progress;

    /// <summary>Raised when the run has finished, so the dialog can close itself. A dialog that
    /// stayed up showing a finished bar would need a second dismissal for no reason.</summary>
    public event Action? Finished;

    /// <summary>Puts the dialog into its running state without a run, so the UI harness can
    /// photograph a state it otherwise only has while something slow is happening.</summary>
    internal void PoseRunning(TransferProgressViewModel surface)
    {
        Progress = surface;
        IsRunning = true;
    }

    /// <summary>
    /// Runs the sync and keeps the dialog up while it does.
    /// </summary>
    /// <remarks>
    /// The window stays open on purpose. Closing it the instant Sync is pressed leaves the only
    /// account of a long operation in the status bar of a window the user has already looked away
    /// from — and the Cancel button with it.
    /// </remarks>
    public async Task RunAsync()
    {
        if (IsRunning || _run is null) return;

        IsRunning = true;
        try
        {
            await _run(Result, surface => Progress = surface);
        }
        finally
        {
            IsRunning = false;
            Progress = null;
            Finished?.Invoke();
        }
    }

    public string LeftPath { get; }

    public string RightPath { get; }

    public ObservableCollection<SyncActionViewModel> Actions { get; } = [];

    /// <summary>Off by default. Everything else a sync does is additive or reversible in place;
    /// this is the half that takes something away.</summary>
    [ObservableProperty]
    private bool _removeRightOnly;

    [ObservableProperty]
    private string _summary = "";

    /// <summary>What the run will leave alone, or null when it will leave nothing alone. Said
    /// before, because an entry nothing could be compared about is not going to announce itself
    /// afterwards.</summary>
    [ObservableProperty]
    private string? _caveat;

    public bool CanRun => !IsRunning && Actions.Any(a => a.Ticked);

    partial void OnRemoveRightOnlyChanged(bool value) => Rebuild();

    /// <summary>
    /// Re-asks the planner, keeping the ticks the user has already changed.
    /// </summary>
    /// <remarks>
    /// Toggling the deletions checkbox is a different question, not a filter over the same answer:
    /// a right-only folder appears or disappears from the list entirely. Re-planning is how the
    /// dialog and the run stay the same shape, and the ticks are carried across by key so a run of
    /// unticking is not undone by reaching for the checkbox.
    /// </remarks>
    private void Rebuild()
    {
        var kept = Actions
            .Where(a => a.Ticked != a.Action.Ticked)
            .ToDictionary(a => a.Action.RelativeKey, a => a.Ticked, StringComparer.Ordinal);

        var preview = _plan(RemoveRightOnly);

        Actions.Clear();
        foreach (var action in preview.Actions.OrderBy(Order).ThenBy(a => a.RelativeDisplay, StringComparer.OrdinalIgnoreCase))
        {
            var row = new SyncActionViewModel(action);
            if (kept.TryGetValue(action.RelativeKey, out var ticked)) row.Ticked = ticked;
            row.PropertyChanged += (_, e) =>
            {
                if (e.PropertyName == nameof(SyncActionViewModel.Ticked)) Refresh();
            };
            Actions.Add(row);
        }

        Refresh();
        Caveat = preview.UnknownCount > 0
            ? $"{preview.UnknownCount:N0} item(s) could not be compared and will be left alone."
            : null;
    }

    private static int Order(SyncAction action) => action.Kind switch
    {
        SyncActionKind.Copy => 0,
        SyncActionKind.Overwrite => 1,
        _ => 2,
    };

    /// <summary>The headline, rebuilt from what is actually ticked right now.</summary>
    private void Refresh()
    {
        var ticked = Actions.Where(a => a.Ticked).ToList();
        var copies = ticked.Count(a => a.Action.Kind is SyncActionKind.Copy);
        var replaces = ticked.Count(a => a.Action.Kind is SyncActionKind.Overwrite);
        var deletes = ticked.Count(a => a.Action.Kind is SyncActionKind.Delete);

        var parts = new List<string>();
        if (copies > 0) parts.Add($"copy {copies:N0}");
        if (replaces > 0) parts.Add($"replace {replaces:N0}");
        if (deletes > 0) parts.Add($"delete {deletes:N0}");

        var text = parts.Count == 0 ? "Nothing selected" : string.Join(" · ", parts);

        // Withheld rather than approximated the moment one contributing size is unknown: a total
        // that quietly leaves files out is worse than no total, because it looks like one.
        var bytes = TotalBytes(ticked);
        if (bytes is { } total && total > 0) text += $" — {ByteSizeFormatter.Format(total)}";

        Summary = text;
        OnPropertyChanged(nameof(CanRun));
    }

    private static long? TotalBytes(IReadOnlyList<SyncActionViewModel> ticked)
    {
        long total = 0;
        foreach (var row in ticked)
        {
            if (row.Action.Kind is SyncActionKind.Delete) continue;
            if (row.Action.Bytes is not { } bytes) return null;
            total += bytes;
        }
        return total;
    }

    /// <summary>What the run will be handed: the planner's own preview with this dialog's ticks on
    /// it, so nothing between here and the executors has to know what a checkbox is.</summary>
    public SyncPreview Result =>
        SyncPlanner.WithTicks(
            _plan(RemoveRightOnly),
            Actions.Where(a => a.Ticked).Select(a => a.Action.RelativeKey).ToHashSet(StringComparer.Ordinal));
}
