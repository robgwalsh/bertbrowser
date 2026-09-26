using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A folder sync in the undo history: copies and removals at once, reversed and redone together by
/// <see cref="SyncRunner"/>. Never retried elevated, as a sync never was.
/// </summary>
public sealed class SyncRecord : IUndoableRecord
{
    private readonly SyncOutcome _outcome;

    private SyncRecord(SyncOutcome outcome, bool isApplied, string description)
    {
        _outcome = outcome;
        IsApplied = isApplied;
        Description = description;
    }

    public static SyncRecord? For(SyncOutcome outcome)
    {
        if (!outcome.CanUndo) return null;

        var into = outcome.Copies.Count > 0
            ? $" into {UndoText.FolderName(outcome.Copies[0].DestinationDirectory)}"
            : "";
        return new SyncRecord(outcome, isApplied: true, $"Sync {UndoText.Items(Count(outcome))}{into}");
    }

    private static int Count(SyncOutcome outcome) =>
        outcome.Copies.Sum(c => c.Completed.Count) + outcome.Removals.Deleted.Count;

    public UndoKind Kind => UndoKind.Sync;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => Count(_outcome);

    public IReadOnlyList<UndoItemDetail> Details =>
    [
        .. _outcome.Copies.SelectMany(c => c.Completed)
            .Select(c => new UndoItemDetail(c.SourcePath, c.FinalPath, c.IsDirectory)),
        .. _outcome.Removals.Deleted
            .Select(d => new UndoItemDetail(d.SourcePath, d.RecycledPath is not null ? "Recycle Bin" : "BertBrowser's holding folder", d.IsDirectory)),
    ];

    public IReadOnlyList<HeldEntry> Held =>
        IsApplied
            ?
            [
                .. _outcome.Copies.SelectMany(c => c.Completed)
                    .Where(c => c.DisplacedStagePath is not null)
                    .Select(c => new HeldEntry(c.DisplacedStagePath!, c.FinalPath, Directory.Exists(c.DisplacedStagePath))),
                .. _outcome.Removals.Deleted
                    .Where(d => d.StagedPath is not null)
                    .Select(d => new HeldEntry(d.StagedPath!, d.SourcePath, d.IsDirectory)),
            ]
            : [];

    public async Task<StepResult> StepAsync(IUndoHost host)
    {
        if (IsApplied)
        {
            var undo = await Task.Run(() => host.Sync.Undo(_outcome));
            return new StepResult(
                new SyncRecord(undo.Reverted, isApplied: false, Description),
                new StepReport(Count(undo.Reverted), undo.Failed),
                RefreshFor(_outcome));
        }

        var whole = new TransferPlan(
            TransferVerb.Copy,
            _outcome.Copies.Count > 0 ? _outcome.Copies[0].DestinationDirectory : "",
            [.. _outcome.Copies.SelectMany(c => c.Completed)
                .Select(c => new PlannedTransfer(c.SourcePath, c.IsDirectory, c.FinalPath, false))],
            []);

        var progress = host.BeginProgress(whole, $"Redoing — {Description}…");
        SyncOutcome redone;
        try
        {
            redone = await Task.Run(() => host.Sync.Redo(_outcome, progress: progress));
        }
        finally
        {
            host.EndProgress();
        }

        redone = redone with { Copies = [.. redone.Copies.Select(TransferExecutor.StampCopies)] };
        var failures = redone.Copies.SelectMany(c => c.Failed.Select(f => f.Message)
                .Concat(c.Skipped.Select(s => $"{Path.GetFileName(s)}: its old place is taken now, so it was left where it is.")))
            .Concat(redone.Removals.Failed.Select(f => f.Message))
            .ToList();

        return new StepResult(
            new SyncRecord(redone, isApplied: true, Description),
            new StepReport(Count(redone), failures),
            RefreshFor(redone));
    }

    public void Retire()
    {
        if (IsApplied) SyncRunner.Retire(_outcome);
    }

    private static UndoRefresh RefreshFor(SyncOutcome outcome) =>
        UndoRefresh.Of(outcome.Copies
            .SelectMany(c => c.Completed.Select(t => Path.GetDirectoryName(t.FinalPath)))
            .Concat(outcome.Copies.Select(c => (string?)c.DestinationDirectory))
            .Concat(outcome.Removals.Deleted.Select(d => Path.GetDirectoryName(d.SourcePath)))) with
        {
            RescanCompare = true,
        };
}
