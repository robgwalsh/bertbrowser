using BertBrowser.Core.Services.Delete;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A delete in the undo history — to the Recycle Bin or into the holding folder. A permanent delete
/// never gets one: there is nothing left to put back.
/// </summary>
/// <remarks>
/// Only items this app holds count against the budget. What went to the Recycle Bin is held by
/// Windows, under Windows' own limits, and retiring the entry leaves it there.
/// </remarks>
public sealed class DeleteRecord : IUndoableRecord
{
    private readonly DeleteOutcome _outcome;

    private DeleteRecord(DeleteOutcome outcome, bool isApplied, string description)
    {
        _outcome = outcome;
        IsApplied = isApplied;
        Description = description;
    }

    /// <param name="description">What to call it, when it was not a delete to the user — a drag out
    /// of the app is carried out as one.</param>
    public static DeleteRecord? For(DeleteOutcome outcome, string? description = null)
    {
        if (!outcome.CanUndo) return null;

        // Only what can come back is worth recording: an item the bin erased rather than holding
        // would fail every undo, and a redo has nothing to redo for it.
        var recoverable = outcome with { Deleted = [.. outcome.Deleted.Where(d => d.IsRecoverable)] };
        return new DeleteRecord(
            recoverable,
            isApplied: true,
            description ?? $"Delete {UndoText.Named([.. recoverable.Deleted.Select(d => d.SourcePath)])}");
    }

    public DeleteOutcome Outcome => _outcome;

    public UndoKind Kind => UndoKind.Delete;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => _outcome.Deleted.Count;

    public IReadOnlyList<UndoItemDetail> Details =>
        [.. _outcome.Deleted.Select(d => new UndoItemDetail(
            d.SourcePath,
            d.RecycledPath is not null ? "Recycle Bin" : "BertBrowser's holding folder",
            d.IsDirectory))];

    public IReadOnlyList<HeldEntry> Held =>
        IsApplied
            ? [.. _outcome.Deleted
                .Where(d => d.StagedPath is not null)
                .Select(d => new HeldEntry(d.StagedPath!, d.SourcePath, d.IsDirectory))]
            : [];

    public Task<StepResult> StepAsync(IUndoHost host) => IsApplied ? UndoAsync(host) : RedoAsync(host);

    private async Task<StepResult> UndoAsync(IUndoHost host)
    {
        var (result, note) = await host.ElevateDeleteUndoAsync(
            _outcome, await Task.Run(() => host.Deletes.Undo(_outcome)));

        var next = new DeleteRecord(
            _outcome with { Deleted = result.Reverted, Failed = [], StagingDirectories = [] },
            isApplied: false,
            Description);

        return new StepResult(
            next,
            new StepReport(result.Reverted.Count, [.. result.Failed.Select(f => f.Message)], note),
            UndoRefresh.Of(_outcome.Deleted.Select(d => Path.GetDirectoryName(d.SourcePath))));
    }

    private async Task<StepResult> RedoAsync(IUndoHost host)
    {
        var plan = DeleteRedo.PlanFor(_outcome.Deleted);
        var (outcome, note) = await host.ElevateAsync(plan, await Task.Run(() => host.Deletes.Execute(plan)));

        // Whatever the executor could not hold is not something a later undo could bring back.
        var next = new DeleteRecord(
            outcome with { Deleted = [.. outcome.Deleted.Where(d => d.IsRecoverable)] },
            isApplied: true,
            Description);

        var refresh = UndoRefresh.Of(plan.Deletions.Select(d => d.ParentPath)) with { Vacated = outcome.Deleted };
        return new StepResult(
            next,
            new StepReport(next.ItemCount, [.. outcome.Failed.Select(f => f.Message)], note),
            refresh);
    }

    public void Retire()
    {
        if (IsApplied) DeleteExecutor.CommitStaging(_outcome);
    }
}
