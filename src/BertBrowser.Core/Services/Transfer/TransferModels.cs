namespace BertBrowser.Core.Services.Transfer;

/// <summary>What a drop does with its sources.</summary>
public enum TransferVerb
{
    /// <summary>Relocate the sources. Destructive to the source location, so it is undoable.</summary>
    Move,

    /// <summary>Duplicate the sources. Purely additive — it never removes or overwrites anything,
    /// which is why <see cref="ConflictResolution.Replace"/> is not offered for it.</summary>
    Copy,
}

/// <summary>Why the planner refused to transfer one source. Everything except
/// <see cref="MovesWithAncestor"/> and <see cref="AlreadyInDestination"/> is a real problem worth
/// telling the user about; those two are ordinary no-ops.</summary>
public enum TransferRejection
{
    /// <summary>The source disappeared between selection and drop.</summary>
    SourceMissing,

    /// <summary>Drive roots and volume roots cannot be relocated.</summary>
    SourceIsRoot,

    /// <summary>The destination does not exist.</summary>
    DestinationMissing,

    /// <summary>The destination path is a file, not a folder.</summary>
    DestinationNotDirectory,

    /// <summary>Dropping a folder onto itself.</summary>
    DestinationIsSource,

    /// <summary>Dropping a folder into its own subtree — the case that eats directory trees.</summary>
    DestinationInsideSource,

    /// <summary>A move whose source already sits in the destination folder: nothing to do.</summary>
    AlreadyInDestination,

    /// <summary>An ancestor of this source is also being transferred, so it travels along with it.
    /// Transferring both would leave a dangling path once the ancestor has moved.</summary>
    MovesWithAncestor,
}

/// <summary>How to settle a name that already exists in the destination.</summary>
public enum ConflictResolution
{
    /// <summary>Leave the source where it is.</summary>
    Skip,

    /// <summary>Transfer under a generated "name (2)" style name. Never touches the existing entry.</summary>
    KeepBoth,

    /// <summary>Take over the existing name. The displaced entry is moved to a staging folder
    /// rather than deleted, so an undo can put it back. Move only.</summary>
    Replace,

    /// <summary>
    /// Take over the existing name on a <b>copy</b>. Writes exactly what
    /// <see cref="Replace"/> writes, through the same staging folder.
    /// </summary>
    /// <remarks>
    /// <para>
    /// What separates it from <see cref="Replace"/> is not the writing but the undo. A copy's
    /// outcome is not undoable — see <see cref="TransferOutcome.CanUndo"/> — so a copy that
    /// displaced something would leave that entry in a hidden folder with no record pointing at it:
    /// never committed, never purged, and gone as far as the user could ever tell. Which is why
    /// <see cref="Replace"/> is still refused for a copy, and why this exists as a separate value
    /// rather than as a relaxation of that rule.
    /// </para>
    /// <para>
    /// Only a caller that keeps the outcome and can undo it may ask for this. Today that is a
    /// folder sync, which does both.
    /// </para>
    /// </remarks>
    Overwrite,
}

/// <summary>One source the planner accepted, with the name it would land under.</summary>
/// <param name="SourcePath">Full path of the item to transfer.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="DestinationPath">Where it lands when nothing is in the way.</param>
/// <param name="Conflicts">True when <paramref name="DestinationPath"/> is already taken.</param>
public sealed record PlannedTransfer(
    string SourcePath,
    bool IsDirectory,
    string DestinationPath,
    bool Conflicts)
{
    public string Name => Path.GetFileName(SourcePath);
}

/// <param name="SourcePath">The source the planner refused.</param>
/// <param name="Reason">Why.</param>
/// <param name="Message">User-facing explanation.</param>
public sealed record RejectedTransfer(string SourcePath, TransferRejection Reason, string Message)
{
    /// <summary>True for refusals that are ordinary no-ops rather than something to report.</summary>
    public bool IsBenign =>
        Reason is TransferRejection.AlreadyInDestination or TransferRejection.MovesWithAncestor;
}

/// <summary>The validated outcome of asking "what would dropping these here do?".</summary>
public sealed record TransferPlan(
    TransferVerb Verb,
    string DestinationDirectory,
    IReadOnlyList<PlannedTransfer> Transfers,
    IReadOnlyList<RejectedTransfer> Rejected)
{
    /// <summary>True when the drop would actually transfer something — the gate for allowing a drop.</summary>
    public bool HasWork => Transfers.Count > 0;

    public IReadOnlyList<PlannedTransfer> Conflicts =>
        Transfers.Where(t => t.Conflicts).ToList();

    /// <summary>Refusals worth surfacing; no-ops are filtered out.</summary>
    public IReadOnlyList<RejectedTransfer> Problems =>
        Rejected.Where(r => !r.IsBenign).ToList();

    public static TransferPlan Empty(TransferVerb verb, string destination) =>
        new(verb, destination, Array.Empty<PlannedTransfer>(), Array.Empty<RejectedTransfer>());
}

/// <summary>One source that made it onto disk at its new location.</summary>
/// <param name="SourcePath">Where it came from — the undo target.</param>
/// <param name="FinalPath">Where it ended up, after any conflict resolution.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="DisplacedStagePath">When the transfer replaced an existing entry, the staging path
/// that entry was moved aside to; null otherwise. Undo restores from here.</param>
public sealed record CompletedTransfer(
    string SourcePath,
    string FinalPath,
    bool IsDirectory,
    string? DisplacedStagePath);

/// <param name="SourcePath">The source that could not be transferred.</param>
/// <param name="Message">The failure, phrased for the status bar.</param>
/// <param name="AccessDenied">Windows refused permission, rather than the item being missing, in
/// use, or on the wrong volume. The one failure an administrator token could fix, and therefore the
/// only one the elevated retry is ever offered for.</param>
public sealed record FailedTransfer(string SourcePath, string Message, bool AccessDenied = false);

/// <summary>What actually happened on disk. <see cref="Completed"/> doubles as the undo record.</summary>
/// <param name="StagingDirectories">Every staging folder this run created, so the whole lot can be
/// discarded in one go once the transfer can no longer be undone.</param>
/// <param name="Cancelled">True when the user stopped the transfer part-way. Without it a cancelled
/// run is indistinguishable from an empty plan: the items that never ran appear in neither
/// <paramref name="Completed"/>, <paramref name="Skipped"/> nor <paramref name="Failed"/>.</param>
/// <remarks>
/// A single run only ever creates one staging folder, so a list looks like one too many — but an
/// outcome is not always a single run. Two of them can be merged into one, which is what the
/// elevated retry does with the pass that failed on permissions and the pass that did not, and there
/// is nowhere to put the second folder if this is a <c>string?</c>. Whichever one was dropped would
/// then never be committed and never purged: the user's displaced folder would stay hidden on disk
/// for good, with no record pointing at it. <c>DeleteOutcome</c> has carried a list for the same
/// reason since it was written.
/// </remarks>
public sealed record TransferOutcome(
    TransferVerb Verb,
    string DestinationDirectory,
    IReadOnlyList<CompletedTransfer> Completed,
    IReadOnlyList<string> Skipped,
    IReadOnlyList<FailedTransfer> Failed,
    IReadOnlyList<string> StagingDirectories,
    bool Cancelled = false)
{
    /// <summary>Only a move is worth undoing: a copy adds without removing or overwriting.
    /// A cancelled move still undoes — what got across is what goes back.</summary>
    public bool CanUndo => Verb == TransferVerb.Move && Completed.Count > 0;
}

/// <summary>
/// Progress while a transfer runs. Byte-level, because item counts say nothing useful about a
/// single large file — "Copying 1 of 1" is what ten silent minutes used to look like.
/// </summary>
/// <param name="Done">Items finished.</param>
/// <param name="Total">Items in the plan.</param>
/// <param name="CurrentName">The item in flight; empty on the terminal report.</param>
/// <param name="BytesDone">Bytes written so far across the whole plan. A move within one volume is
/// a rename, so it moves no bytes and this stays at zero — which is correct, not a stall.</param>
/// <param name="CurrentItemBytes">Bytes written into the item in flight.</param>
/// <param name="CurrentItemTotal">That item's size as the OS reports it; 0 when not yet known.</param>
/// <remarks>The plan's byte total is deliberately absent: it comes from the directory size index
/// rather than from disk, which is the caller's business and keeps this executor free of any
/// database dependency.</remarks>
public sealed record TransferProgress(
    int Done,
    int Total,
    string CurrentName,
    long BytesDone = 0,
    long CurrentItemBytes = 0,
    long CurrentItemTotal = 0);
