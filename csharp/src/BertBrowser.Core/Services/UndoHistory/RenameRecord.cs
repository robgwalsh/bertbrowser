using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A rename in the undo history. A rename is its own inverse, so undo and redo are the same step:
/// run the outcome's reversal, and keep what that run completed as the next record.
/// </summary>
/// <remarks>
/// It holds nothing — its temporary names are gone by the time it finishes — so it costs the budget
/// nothing and retiring it does nothing.
/// </remarks>
public sealed class RenameRecord : IUndoableRecord
{
    /// <summary>The last run, in the direction it went. Undone, that is the reversal, so its
    /// "source" is where the item was renamed <em>to</em>.</summary>
    private readonly RenameOutcome _lastRun;

    private RenameRecord(RenameOutcome lastRun, bool isApplied, string description)
    {
        _lastRun = lastRun;
        IsApplied = isApplied;
        Description = description;
    }

    public static RenameRecord? For(RenameOutcome outcome)
    {
        if (!outcome.CanUndo) return null;

        var description = outcome.Completed.Count == 1
            ? $"Rename '{Path.GetFileName(outcome.Completed[0].SourcePath)}' to '{Path.GetFileName(outcome.Completed[0].FinalPath)}'"
            : $"Rename {UndoText.Items(outcome.Completed.Count)}";
        return new RenameRecord(outcome, isApplied: true, description);
    }

    public UndoKind Kind => UndoKind.Rename;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => _lastRun.Completed.Count;

    /// <summary>Always old name → new name, whichever way the last run went.</summary>
    public IReadOnlyList<UndoItemDetail> Details =>
        [.. _lastRun.Completed.Select(c => IsApplied
            ? new UndoItemDetail(c.SourcePath, c.FinalPath, c.IsDirectory)
            : new UndoItemDetail(c.FinalPath, c.SourcePath, c.IsDirectory))];

    public IReadOnlyList<HeldEntry> Held => [];

    public async Task<StepResult> StepAsync(IUndoHost host)
    {
        var plan = RenameExecutor.UndoPlan(_lastRun);
        var (result, note) = await host.ElevateAsync(plan, await Task.Run(() => host.Renames.Execute(plan)));

        return new StepResult(
            new RenameRecord(result, !IsApplied, Description),
            new StepReport(result.Completed.Count, [.. result.Failed.Select(f => f.Message)], note),
            UndoRefresh.Of(plan.Renames.Select(r => Path.GetDirectoryName(r.SourcePath))) with
            {
                Renamed = result.Completed,
            });
    }

    public void Retire()
    {
    }
}
