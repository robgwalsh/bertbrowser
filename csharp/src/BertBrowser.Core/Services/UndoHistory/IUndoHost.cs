using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Timestamps;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// What a record needs from the app to take a step: the executors, the elevated retry, and a
/// progress surface for a redo that writes bytes.
/// </summary>
/// <remarks>
/// <para>
/// <b>The elevated retry stays in the App</b>, where the prompt and the helper process live, and
/// comes in through here. There is deliberately no member for retrying the undo of a <em>copy</em>:
/// <c>ElevationHost.UndoTransfer</c> hardcodes a move, so handed a paste's outcome it would move the
/// pasted files back into the folder they came from instead of removing them — corrupting both sides.
/// <see cref="ElevateMoveUndoAsync"/> refuses anything but a move for the same reason.
/// </para>
/// <para>
/// Every executor call a record makes is synchronous disk work, so records run them on the thread
/// pool and come back to the caller's context for the elevation calls.
/// </para>
/// </remarks>
public interface IUndoHost
{
    TransferExecutor Transfers { get; }

    DeleteExecutor Deletes { get; }

    RenameExecutor Renames { get; }

    ArchiveEditExecutor ArchiveEdits { get; }

    /// <summary>No elevated retry goes with this one, for the reason archives have none.</summary>
    MetadataEditExecutor MetadataEdits { get; }

    TimestampExecutor Timestamps { get; }

    SyncRunner Sync { get; }

    Task<Elevated<TransferOutcome>> ElevateAsync(
        TransferPlan plan, TransferOutcome outcome, IReadOnlyDictionary<string, ConflictResolution> resolutions);

    Task<Elevated<DeleteOutcome>> ElevateAsync(DeletePlan plan, DeleteOutcome outcome);

    Task<Elevated<RenameOutcome>> ElevateAsync(RenamePlan plan, RenameOutcome outcome);

    /// <summary>The retry for putting a move back. Throws for anything but a move.</summary>
    Task<Elevated<TransferUndoResult>> ElevateMoveUndoAsync(TransferOutcome outcome, TransferUndoResult result);

    Task<Elevated<DeleteUndoResult>> ElevateDeleteUndoAsync(DeleteOutcome outcome, DeleteUndoResult result);

    /// <summary>Puts a progress surface up for a redo that writes, and returns its sink. Paired
    /// with <see cref="EndProgress"/>.</summary>
    IProgress<TransferProgress>? BeginProgress(TransferPlan plan, string headline);

    void EndProgress();
}

/// <summary>
/// A host with no elevation and no progress surface: every retry is a pass-through. For tests, and
/// for anything that runs where there is nobody to ask.
/// </summary>
public sealed class PlainUndoHost(
    TransferExecutor transfers,
    DeleteExecutor deletes,
    RenameExecutor renames,
    ArchiveEditExecutor archiveEdits) : IUndoHost
{
    public TransferExecutor Transfers { get; } = transfers;

    public DeleteExecutor Deletes { get; } = deletes;

    public RenameExecutor Renames { get; } = renames;

    public ArchiveEditExecutor ArchiveEdits { get; } = archiveEdits;

    public MetadataEditExecutor MetadataEdits { get; } = new();

    public TimestampExecutor Timestamps { get; } = new();

    public SyncRunner Sync { get; } = new(transfers, deletes);

    public Task<Elevated<TransferOutcome>> ElevateAsync(
        TransferPlan plan, TransferOutcome outcome, IReadOnlyDictionary<string, ConflictResolution> resolutions) =>
        Task.FromResult(new Elevated<TransferOutcome>(outcome, ""));

    public Task<Elevated<DeleteOutcome>> ElevateAsync(DeletePlan plan, DeleteOutcome outcome) =>
        Task.FromResult(new Elevated<DeleteOutcome>(outcome, ""));

    public Task<Elevated<RenameOutcome>> ElevateAsync(RenamePlan plan, RenameOutcome outcome) =>
        Task.FromResult(new Elevated<RenameOutcome>(outcome, ""));

    public Task<Elevated<TransferUndoResult>> ElevateMoveUndoAsync(TransferOutcome outcome, TransferUndoResult result)
    {
        if (outcome.Verb != TransferVerb.Move)
            throw new InvalidOperationException("Only a move's undo can be retried elevated.");
        return Task.FromResult(new Elevated<TransferUndoResult>(result, ""));
    }

    public Task<Elevated<DeleteUndoResult>> ElevateDeleteUndoAsync(DeleteOutcome outcome, DeleteUndoResult result) =>
        Task.FromResult(new Elevated<DeleteUndoResult>(result, ""));

    public IProgress<TransferProgress>? BeginProgress(TransferPlan plan, string headline) => null;

    public void EndProgress()
    {
    }
}
