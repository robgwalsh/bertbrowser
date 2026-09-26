using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A move, or a merged paste, in the undo history.
/// </summary>
/// <remarks>
/// <para>
/// Applied, it holds the outcome the executor returned, and with it whatever a Replace displaced
/// into staging. Undone, it holds only the completions the undo actually reversed, with no staging:
/// the undo has put the displaced entries back, and the redo will displace them again under
/// <see cref="TransferRedo"/>'s rules.
/// </para>
/// <para>
/// <b>A paste's undo is never retried elevated.</b> See <see cref="IUndoHost"/>: the helper's undo
/// would treat it as a move.
/// </para>
/// </remarks>
public sealed class TransferRecord : IUndoableRecord
{
    private readonly TransferOutcome _outcome;

    private TransferRecord(TransferOutcome outcome, bool isApplied, string description)
    {
        _outcome = outcome;
        IsApplied = isApplied;
        Description = description;
    }

    /// <summary>The record for a move, or a merged paste the caller undertakes to keep; null for
    /// anything the history cannot take back.</summary>
    public static TransferRecord? For(TransferOutcome outcome)
    {
        if (!outcome.CanUndo && !outcome.CanUndoCopy) return null;

        var where = UndoText.FolderName(outcome.DestinationDirectory);
        var count = UndoText.Items(outcome.Completed.Count);
        var description = outcome.Verb == TransferVerb.Move
            ? $"Move {count} to {where}"
            : $"Paste {count} into {where}";
        return new TransferRecord(outcome, isApplied: true, description);
    }

    public TransferOutcome Outcome => _outcome;

    public UndoKind Kind => _outcome.Verb == TransferVerb.Move ? UndoKind.Move : UndoKind.Paste;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => _outcome.Completed.Count;

    public IReadOnlyList<UndoItemDetail> Details =>
        [.. _outcome.Completed.Select(c => new UndoItemDetail(c.SourcePath, c.FinalPath, c.IsDirectory))];

    public IReadOnlyList<HeldEntry> Held =>
        IsApplied
            ? [.. _outcome.Completed
                .Where(c => c.DisplacedStagePath is not null)
                .Select(c => new HeldEntry(c.DisplacedStagePath!, c.FinalPath, Directory.Exists(c.DisplacedStagePath)))]
            : [];

    public Task<StepResult> StepAsync(IUndoHost host) => IsApplied ? UndoAsync(host) : RedoAsync(host);

    private async Task<StepResult> UndoAsync(IUndoHost host)
    {
        TransferUndoResult result;
        var note = "";
        if (_outcome.Verb == TransferVerb.Copy)
        {
            result = await Task.Run(() => host.Transfers.UndoCopies(_outcome));
        }
        else
        {
            (result, note) = await host.ElevateMoveUndoAsync(
                _outcome, await Task.Run(() => host.Transfers.Undo(_outcome)));
        }

        var next = new TransferRecord(
            _outcome with
            {
                Completed = result.Reverted,
                Skipped = [],
                Failed = [],
                StagingDirectories = [],
                Cancelled = false,
            },
            isApplied: false,
            Description);

        return new StepResult(
            next,
            new StepReport(result.Reverted.Count, [.. result.Failed.Select(f => f.Message)], note),
            RefreshFor(_outcome));
    }

    private async Task<StepResult> RedoAsync(IUndoHost host)
    {
        var (plan, resolutions) = TransferRedo.PlanFor(
            _outcome.Verb, _outcome.DestinationDirectory, _outcome.Completed, _outcome.PrunedDirectories);

        var progress = host.BeginProgress(plan, $"Redoing — {Description}…");
        TransferOutcome outcome;
        try
        {
            outcome = await Task.Run(() => host.Transfers.Execute(plan, resolutions, progress: progress));
        }
        finally
        {
            host.EndProgress();
        }

        var (merged, note) = await host.ElevateAsync(plan, outcome, resolutions);
        if (merged.Verb == TransferVerb.Copy)
            merged = TransferExecutor.StampCopies(merged) with { CanUndoCopy = true };

        // A redo that moved nothing leaves the entry where it was, so nothing will ever retire this
        // outcome: take away any staging folder it made and emptied again, never one still holding.
        if (merged.Completed.Count == 0) host.Transfers.PurgeStaging(merged);

        var failures = merged.Failed.Select(f => f.Message)
            .Concat(merged.Skipped.Select(s => $"{Path.GetFileName(s)}: its old place is taken now, so it was left where it is."))
            .ToList();

        return new StepResult(
            new TransferRecord(merged, isApplied: true, Description),
            new StepReport(merged.Completed.Count, failures, note),
            RefreshFor(merged));
    }

    public void Retire()
    {
        if (IsApplied) TransferExecutor.CommitStaging(_outcome);
    }

    /// <summary>Both ends of every item, the destination, and the parents of any folder a merge
    /// emptied — the same set a drop refreshes.</summary>
    private static UndoRefresh RefreshFor(TransferOutcome outcome) =>
        UndoRefresh.Of(outcome.Completed
            .SelectMany(c => new[] { Path.GetDirectoryName(c.SourcePath), Path.GetDirectoryName(c.FinalPath) })
            .Concat(outcome.PrunedDirectories.Select(Path.GetDirectoryName))
            .Append(outcome.DestinationDirectory));
}
