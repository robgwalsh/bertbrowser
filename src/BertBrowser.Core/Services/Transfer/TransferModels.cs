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
public sealed record FailedTransfer(string SourcePath, string Message);

/// <summary>What actually happened on disk. <see cref="Completed"/> doubles as the undo record.</summary>
public sealed record TransferOutcome(
    TransferVerb Verb,
    string DestinationDirectory,
    IReadOnlyList<CompletedTransfer> Completed,
    IReadOnlyList<string> Skipped,
    IReadOnlyList<FailedTransfer> Failed,
    string? StagingDirectory)
{
    /// <summary>Only a move is worth undoing: a copy adds without removing or overwriting.</summary>
    public bool CanUndo => Verb == TransferVerb.Move && Completed.Count > 0;
}

/// <summary>Progress for the status bar while a transfer runs.</summary>
public sealed record TransferProgress(int Done, int Total, string CurrentName);
