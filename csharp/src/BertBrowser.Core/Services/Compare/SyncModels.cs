using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Compare;

/// <summary>What a sync would do about one entry.</summary>
public enum SyncActionKind
{
    /// <summary>Write it to a name the right side does not use.</summary>
    Copy,

    /// <summary>Write it over what the right side has, which goes to staging first.</summary>
    Overwrite,

    /// <summary>Remove it from the right side. The only kind that takes anything away.</summary>
    Delete,
}

/// <summary>
/// One thing a sync would do, as the user is shown it before agreeing to any of it.
/// </summary>
/// <param name="RelativeDisplay">Where it sits under the two roots, in the casing the side that
/// has it recorded.</param>
/// <param name="SourcePath">The left-side path to copy from; empty for a delete.</param>
/// <param name="TargetPath">The right-side path being written or removed.</param>
/// <param name="Bytes">What it weighs, or null when nothing measured it. Null is <em>unknown</em>
/// and must render blank — never as zero, and never quietly summed into a total.</param>
/// <param name="Ticked">Whether it runs. A folder carries every one of its own entries, so
/// unticking a folder unticks everything under it.</param>
public sealed record SyncAction(
    string RelativeKey,
    string RelativeDisplay,
    SyncActionKind Kind,
    string SourcePath,
    string TargetPath,
    bool IsDirectory,
    long? Bytes,
    CompareVerdict Verdict,
    bool Ticked)
{
    public string Name => Path.GetFileName(TargetPath);
}

/// <summary>
/// Everything a sync would do, and what it deliberately would not.
/// </summary>
/// <param name="UnknownCount">Entries no verdict could be reached for. They produce no action at
/// all — you cannot sync what you could not compare — and are surfaced as a caveat rather than
/// silently left out of a list that claims to be complete.</param>
public sealed record SyncPreview(
    string LeftPath,
    string RightPath,
    IReadOnlyList<SyncAction> Actions,
    int UnknownCount)
{
    public IReadOnlyList<SyncAction> Ticked => [.. Actions.Where(a => a.Ticked)];

    public int CopyCount => Ticked.Count(a => a.Kind is SyncActionKind.Copy);

    public int OverwriteCount => Ticked.Count(a => a.Kind is SyncActionKind.Overwrite);

    public int DeleteCount => Ticked.Count(a => a.Kind is SyncActionKind.Delete);

    public bool HasWork => Actions.Any(a => a.Ticked);

    /// <summary>
    /// What the ticked copies weigh, or null when any one of them was never measured.
    /// </summary>
    /// <remarks>
    /// Withheld rather than approximated, the same rule the rest of the app follows for a size it
    /// does not have: a total that silently omits the files nobody could measure is worse than no
    /// total, because it looks like one.
    /// </remarks>
    public long? TotalBytes
    {
        get
        {
            long total = 0;
            foreach (var action in Actions)
            {
                if (!action.Ticked || action.Kind is SyncActionKind.Delete) continue;
                if (action.Bytes is not { } bytes) return null;
                total += bytes;
            }
            return total;
        }
    }

    public static SyncPreview Empty(string left, string right) => new(left, right, [], 0);
}

/// <summary>
/// A preview turned into the plans the existing executors already know how to carry out.
/// </summary>
/// <param name="Copies">One plan per destination folder. A <see cref="TransferPlan"/> carries a
/// single destination by construction and a recursive sync writes into many, so the alternative
/// would be a second kind of transfer plan — this way every one of the transfer planner's rules
/// stays in force, per destination, unchanged.</param>
/// <param name="Resolutions">Keyed by the canonical source path, as the executor expects.</param>
/// <param name="Refused">Anything a planner turned down, kept rather than dropped: a sync that
/// silently does less than it showed is the failure worth reporting.</param>
public sealed record SyncPlans(
    IReadOnlyList<TransferPlan> Copies,
    IReadOnlyDictionary<string, ConflictResolution> Resolutions,
    DeletePlan Removals,
    IReadOnlyList<string> Refused)
{
    public int ItemCount => Copies.Sum(c => c.Transfers.Count) + Removals.Deletions.Count;

    public bool HasWork => ItemCount > 0;
}

/// <summary>
/// What a sync did, as the two kinds of record the existing executors produce.
/// </summary>
/// <remarks>
/// It holds a list of copy outcomes for the same reason it holds a list of plans: one destination
/// folder each. Undo walks them in reverse, so a folder is never removed before what was written
/// inside it.
/// </remarks>
public sealed record SyncOutcome(
    IReadOnlyList<TransferOutcome> Copies,
    DeleteOutcome Removals,
    bool Cancelled)
{
    public int CopiedCount => Copies.Sum(c => c.Completed.Count(t => t.DisplacedStagePath is null));

    public int ReplacedCount => Copies.Sum(c => c.Completed.Count(t => t.DisplacedStagePath is not null));

    public int RemovedCount => Removals.Deleted.Count;

    public IReadOnlyList<FailedTransfer> FailedCopies => [.. Copies.SelectMany(c => c.Failed)];

    /// <summary>
    /// Whether the whole run can be put back: the copies through
    /// <see cref="TransferExecutor.UndoCopies"/>, the removals through
    /// <see cref="DeleteExecutor.Undo"/>. A run that did nothing is not undoable, and neither is
    /// one whose removals were erased outright — which is why the delete half asks its own outcome
    /// rather than trusting the mode it was given.
    /// </summary>
    public bool CanUndo =>
        Copies.Any(c => c.Completed.Count > 0) || Removals.CanUndo;

    public static SyncOutcome Empty { get; } = new([], DeleteOutcome.Empty(permanent: false), false);
}
