using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// Running a sync's plans, in order, through the executors that already exist.
/// </summary>
/// <remarks>
/// <para>
/// It opens no file and creates no directory. Every byte goes through
/// <see cref="TransferExecutor"/> and every removal through <see cref="DeleteExecutor"/>, so all
/// of their rules are in force unchanged: the plan is re-validated against live disk at the moment
/// of the write, a displaced entry goes to staging rather than being erased, one item's failure
/// never affects the others, and a cancel leaves nothing half-written.
/// </para>
/// <para>
/// <b>Copies run before removals.</b> A cancel half-way then leaves the right side holding more
/// than it started with rather than less, which is the direction to fail in — the copies can be
/// repeated harmlessly, whereas a delete that ran before its replacement arrived is a gap.
/// </para>
/// </remarks>
public sealed class SyncRunner
{
    private readonly TransferExecutor _transfers;
    private readonly DeleteExecutor _deletes;

    public SyncRunner(TransferExecutor transfers, DeleteExecutor deletes)
    {
        _transfers = transfers;
        _deletes = deletes;
    }

    public SyncOutcome Run(
        SyncPlans plans, CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null)
    {
        ArgumentNullException.ThrowIfNull(plans);

        // One denominator for the whole run, counted before it starts: a bar that restarts at each
        // destination folder would say a recursive sync was nearly done several times over.
        var total = plans.ItemCount;
        var done = 0;
        var cancelled = false;

        var copies = new List<TransferOutcome>();
        foreach (var plan in plans.Copies)
        {
            if (ct.IsCancellationRequested)
            {
                cancelled = true;
                break;
            }

            var outcome = _transfers.Execute(
                plan, plans.Resolutions, ct, Offset(progress, done, total));

            copies.Add(outcome);
            done += plan.Transfers.Count;
            if (outcome.Cancelled) { cancelled = true; break; }
        }

        var removals = DeleteOutcome.Empty(plans.Removals.Permanent);
        if (!cancelled && plans.Removals.HasWork && !ct.IsCancellationRequested)
            removals = _deletes.Execute(plans.Removals, ct, AsDelete(progress, done, total));

        return new SyncOutcome(copies, removals, cancelled || ct.IsCancellationRequested);
    }

    /// <summary>
    /// Puts back everything the run did: the copies through
    /// <see cref="TransferExecutor.UndoCopies"/>, the removals through
    /// <see cref="DeleteExecutor.Undo"/>.
    /// </summary>
    /// <remarks>
    /// In the reverse order they ran, so what was deleted comes back before the copies that may
    /// have replaced it are unwound, and a folder is never removed before what was written inside
    /// it. Each half reports its own failures; neither stops the other, because a sync that could
    /// only be half undone still has the other half worth undoing.
    /// </remarks>
    public SyncUndoResult Undo(SyncOutcome outcome, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(outcome);

        var deleteUndo = outcome.Removals.CanUndo
            ? _deletes.Undo(outcome.Removals, ct)
            : new DeleteUndoResult(0, []);

        var restored = deleteUndo.Restored;
        var failed = new List<string>();
        foreach (var failure in deleteUndo.Failed)
            failed.Add(failure.Message);

        foreach (var copy in outcome.Copies.Reverse())
        {
            var undo = _transfers.UndoCopies(copy, ct);
            restored += undo.Restored;
            foreach (var failure in undo.Failed)
                failed.Add(failure.Message);
        }

        return new SyncUndoResult(restored, failed);
    }

    /// <summary>Commits what the run set aside, once it can no longer be undone. Until this runs,
    /// a replaced file is still on disk in a staging folder and a removed one is still in the
    /// Recycle Bin.</summary>
    public static void Retire(SyncOutcome outcome)
    {
        ArgumentNullException.ThrowIfNull(outcome);

        foreach (var copy in outcome.Copies)
            TransferExecutor.CommitStaging(copy);

        DeleteExecutor.CommitStaging(outcome.Removals);
    }

    private static IProgress<TransferProgress>? Offset(
        IProgress<TransferProgress>? inner, int done, int total) =>
        inner is null ? null : new OffsetTransfer(inner, done, total);

    private static IProgress<DeleteProgress>? AsDelete(
        IProgress<TransferProgress>? inner, int done, int total) =>
        inner is null ? null : new DeleteAsTransfer(inner, done, total);

    private sealed class OffsetTransfer(IProgress<TransferProgress> inner, int done, int total)
        : IProgress<TransferProgress>
    {
        public void Report(TransferProgress value) => inner.Report(value with
        {
            Done = done + value.Done,
            Total = total,
        });
    }

    /// <summary>The removals reported on the same bar as the copies. Byte figures are deliberately
    /// left at zero: a delete moves no bytes, and carrying the copies' running total through would
    /// make the throughput read as though it were still writing.</summary>
    private sealed class DeleteAsTransfer(IProgress<TransferProgress> inner, int done, int total)
        : IProgress<DeleteProgress>
    {
        public void Report(DeleteProgress value) =>
            inner.Report(new TransferProgress(done + value.Done, total, value.CurrentName));
    }
}

/// <summary>Reversing a sync: how many items came back, and what could not.</summary>
public sealed record SyncUndoResult(int Restored, IReadOnlyList<string> Failed);
