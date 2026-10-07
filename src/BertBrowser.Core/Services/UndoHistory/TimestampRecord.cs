using BertBrowser.Core.Services.Timestamps;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A change of dates in the undo history. It holds nothing on disk — the old dates ride in the
/// record — so there is nothing to retire and nothing counted against the history's budget.
/// </summary>
public sealed class TimestampRecord : IUndoableRecord
{
    private readonly IReadOnlyList<StampedEntry> _stamped;

    private TimestampRecord(IReadOnlyList<StampedEntry> stamped, bool isApplied, string description)
    {
        _stamped = stamped;
        IsApplied = isApplied;
        Description = description;
    }

    public static TimestampRecord? For(TimestampOutcome outcome) =>
        outcome.CanUndo
            ? new TimestampRecord(outcome.Stamped, true, outcome.Stamped.Count == 1
                ? $"Change the date of {Path.GetFileName(outcome.Stamped[0].Path)}"
                : $"Change the dates of {outcome.Stamped.Count} items")
            : null;

    public UndoKind Kind => UndoKind.Timestamps;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => _stamped.Count;

    public IReadOnlyList<UndoItemDetail> Details =>
        [.. _stamped.Select(s => new UndoItemDetail(s.Path, s.Path, s.IsDirectory))];

    public IReadOnlyList<HeldEntry> Held => [];

    public async Task<StepResult> StepAsync(IUndoHost host)
    {
        var stamped = _stamped;
        var result = await Task.Run(() => host.Timestamps.Revert(stamped));

        var refresh = UndoRefresh.Of(stamped.Select(s => Path.GetDirectoryName(s.Path)));
        var report = new StepReport(result.Stamped.Count, [.. result.Failed.Select(f => f.Message)]);

        return result.Stamped.Count == 0
            ? new StepResult(this, report, refresh)
            : new StepResult(new TimestampRecord(result.Stamped, !IsApplied, Description), report, refresh);
    }

    public void Retire()
    {
    }
}
