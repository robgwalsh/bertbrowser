using BertBrowser.Core.Paths;

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
    /// Only a caller that keeps the outcome and can undo it may ask for this. Two do: a folder
    /// sync, and a merged drop or paste — which is why <see cref="TransferOutcome.CanUndoCopy"/>
    /// exists and why the shell sets it in the same breath as putting the outcome in the undo slot.
    /// Without it a merge could never bring an outdated file up to date, which is most of the
    /// reason to want one.
    /// </para>
    /// </remarks>
    Overwrite,
}

/// <summary>One side of a clash, as whatever found it already described it.</summary>
public readonly record struct ClashSide(bool IsDirectory, long Bytes, DateTime ModifiedUtc);

/// <summary>
/// Both sides of one name clash, and the two things a user needs told about it.
/// </summary>
/// <param name="Existing">What is already at the destination, or null when the clash is with
/// another item arriving in this same transfer — where nothing is on disk to describe.</param>
/// <remarks>
/// <b>Carried on the plan so the dialog does no disk IO.</b> The rows used to stat both sides in
/// their constructor, on the UI thread, inside <c>ShowDialog</c>. One clash could afford that; a
/// merged folder's worth cannot.
/// </remarks>
public sealed record TransferClash(ClashSide Incoming, ClashSide? Existing)
{
    /// <summary>Same size, same instant: replacing it would write the bytes that are already there,
    /// and keeping both would manufacture a second copy of a file the user already has.</summary>
    public bool Identical { get; private init; }

    /// <summary>The common reason for wanting Replace, and what the dialog highlights.</summary>
    public bool IncomingIsNewer { get; private init; }

    /// <summary>
    /// A clash with something on disk, judged.
    /// </summary>
    /// <remarks>
    /// <b>Strict tolerance, never Loose.</b> <see cref="Compare.CompareEquality"/> offers a
    /// whole-hour forgiveness for FAT volumes that store local time with no zone, and a folder
    /// comparison wants it. Here it would be the wrong way round: forgiving an hour pre-selects
    /// <see cref="ConflictResolution.Skip"/> on a file that is genuinely an hour newer, so the user
    /// asks for a merge and silently does not get one. The two-second granularity is kept, because
    /// a file copied onto a USB stick really does come back up to two seconds off.
    /// </remarks>
    public static TransferClash Between(ClashSide incoming, ClashSide existing)
    {
        var comparable = !incoming.IsDirectory && !existing.IsDirectory &&
            incoming.ModifiedUtc != default && existing.ModifiedUtc != default;
        var byTime = comparable
            ? Compare.CompareEquality.CompareTimes(
                incoming.ModifiedUtc, existing.ModifiedUtc, Compare.CompareTolerance.Strict)
            : 0;

        return new TransferClash(incoming, existing)
        {
            Identical = comparable && byTime == 0 && incoming.Bytes == existing.Bytes,
            IncomingIsNewer = comparable && byTime > 0,
        };
    }

    /// <summary>A clash with another item arriving in this same transfer.</summary>
    public static TransferClash Arriving(ClashSide incoming) => new(incoming, null);
}

/// <summary>
/// What a clash should be answered with before anyone has said otherwise.
/// </summary>
/// <remarks>
/// In Core, and used by both the dialog and the headless prompt, so an unattended path and a
/// visible one cannot start from different answers.
/// </remarks>
public static class ConflictDefaults
{
    /// <summary>
    /// Keep both, except where the two sides are the same file — there, Skip.
    /// </summary>
    /// <remarks>
    /// Keep both is the one answer that can neither lose the incoming copy nor displace what is
    /// already there, which is why it is the default everywhere else. Applied to a byte-identical
    /// file it is simply wrong: it manufactures <c>img (2).jpg</c> beside an <c>img.jpg</c> the user
    /// already has, and a merge of two mostly-alike folders would do it hundreds of times.
    /// </remarks>
    public static ConflictResolution For(PlannedTransfer transfer) =>
        transfer.Clash is { Identical: true }
            ? ConflictResolution.Skip
            : ConflictResolution.KeepBoth;

    /// <summary>
    /// Whether Replace may be offered for this item at all.
    /// </summary>
    /// <remarks>
    /// A move has always been allowed to displace. A copy has not, because
    /// <see cref="TransferOutcome.CanUndo"/> is false for one and a displaced entry would sit in
    /// staging with no record pointing at it. Inside a merge that reasoning no longer applies: the
    /// caller keeps the outcome and can reverse it, so Replace is offered — and without it a merge
    /// could never bring an outdated file up to date, which is most of the reason to want one. A
    /// folder too large to merge keeps the capabilities it had, because the point of that fallback
    /// is to be the behaviour that already existed rather than a third one.
    /// </remarks>
    public static bool AllowsReplace(TransferPlan plan, PlannedTransfer transfer) =>
        plan.Verb == TransferVerb.Move || (plan.IsMerged && !transfer.MergeDeclined);

    /// <summary>
    /// What a chosen "Replace" actually means here. A copy says
    /// <see cref="ConflictResolution.Overwrite"/>, which writes the same bytes through the same
    /// staging folder but is only ever asked for by a caller that keeps the outcome —
    /// <see cref="TransferExecutor"/> still rewrites a plain <see cref="ConflictResolution.Replace"/>
    /// to Keep both on a copy, and must keep doing so.
    /// </summary>
    public static ConflictResolution ReplaceMeans(TransferPlan plan) =>
        plan.Verb == TransferVerb.Move ? ConflictResolution.Replace : ConflictResolution.Overwrite;
}

/// <summary>One source the planner accepted, with the name it would land under.</summary>
/// <param name="SourcePath">Full path of the item to transfer.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="DestinationPath">Where it lands when nothing is in the way.</param>
/// <param name="Conflicts">True when <paramref name="DestinationPath"/> is already taken.</param>
/// <param name="KnownBytes">The source's size, when whatever produced this item already knew it.
/// Set only by <see cref="TransferMergeExpander"/>, whose enumeration gets a file's length free from
/// the find data — so a merge of four thousand files costs the estimate no stats at all. Null
/// everywhere else, and for a directory, which is sized from the index rather than from disk.</param>
/// <param name="Clash">Both sides of this item's clash, when something already knew them. Null on a
/// plan that never went through <see cref="TransferMergeExpander"/>, which is every plan the
/// elevation host, a sync, or an archive operation builds.</param>
/// <param name="MergeDeclined">This folder clashes with a folder and would have been merged, but the
/// overlap was too large to put to the user item by item. It keeps the whole-folder question it
/// always had — see <see cref="TransferMergeLimits"/>.</param>
public sealed record PlannedTransfer(
    string SourcePath,
    bool IsDirectory,
    string DestinationPath,
    bool Conflicts,
    long? KnownBytes = null,
    TransferClash? Clash = null,
    bool MergeDeclined = false)
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
    /// <summary>
    /// Source directories <see cref="TransferMergeExpander"/> opened rather than moved whole,
    /// deepest first. A move removes each one that is empty when the run ends; a copy leaves them
    /// all. Empty on every plan that was not expanded.
    /// </summary>
    /// <remarks>
    /// An <c>init</c> property rather than a positional parameter so every existing construction —
    /// the elevation host's, the elevated retry's, a sync's, the three synthetic plans in
    /// <c>ShellViewModel</c> — keeps compiling and correctly gets an empty list. None of them merges,
    /// and none of them should prune.
    /// </remarks>
    public IReadOnlyList<string> PruneDirectories { get; init; } = [];

    /// <summary>The names of the folders this plan merges into a folder of the same name. Non-empty
    /// is what "this plan was expanded" means.</summary>
    public IReadOnlyList<string> MergedFolders { get; init; } = [];

    public bool IsMerged => MergedFolders.Count > 0;

    /// <summary>True when the drop would actually transfer something — the gate for allowing a drop.</summary>
    public bool HasWork => Transfers.Count > 0;

    public IReadOnlyList<PlannedTransfer> Conflicts =>
        Transfers.Where(t => t.Conflicts).ToList();

    /// <summary>
    /// The earlier item in this same plan that already claimed <paramref name="transfer"/>'s
    /// destination name, or null when nothing here did.
    /// </summary>
    /// <remarks>
    /// <b>A clash is not always with something on disk.</b> Two sources from different folders can
    /// carry the same name — <c>TransferPlannerTests.TwoSourcesWithTheSameName_SecondOneConflicts</c>
    /// pins that the second is flagged — and at planning time nothing is at that destination yet.
    /// Asking disk about it answers "missing", which is true and useless: what the user needs to be
    /// told is which other item in this drop they are choosing between. Both can also hold at once,
    /// and then the entry on disk is the one worth describing, since that is what a Replace would
    /// displace.
    /// </remarks>
    public PlannedTransfer? EarlierClaimantOf(PlannedTransfer transfer)
    {
        var key = PathKey.Canonicalize(transfer.DestinationPath);
        foreach (var other in Transfers)
        {
            if (ReferenceEquals(other, transfer)) break;
            if (PathKey.Canonicalize(other.DestinationPath) == key) return other;
        }
        return null;
    }

    /// <summary>
    /// How to name an item to the user: its path relative to where the drop lands.
    /// </summary>
    /// <remarks>
    /// A leaf name for everything the planner produced, since those all land directly in the
    /// destination folder — so nothing that existed before this reads any differently. An expanded
    /// merge is what needs the rest: forty rows called <c>config.json</c> are forty rows the user
    /// cannot tell apart, and the folder they came from is the entire difference between them.
    /// </remarks>
    public string LabelFor(PlannedTransfer transfer)
    {
        try
        {
            var relative = Path.GetRelativePath(DestinationDirectory, transfer.DestinationPath);
            return relative.StartsWith("..", StringComparison.Ordinal) ? transfer.Name : relative;
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return transfer.Name;
        }
    }

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
    /// <summary>
    /// Source directories this run removed because a merge had emptied them, deepest first.
    /// <see cref="TransferExecutor.Undo"/> recreates exactly these, and nothing else, before putting
    /// anything back — otherwise every restore into one of them fails the "its original folder is
    /// gone" guard, which exists to refuse restoring into a folder the <em>user</em> deleted
    /// meanwhile and must keep meaning only that.
    /// </summary>
    public IReadOnlyList<string> PrunedDirectories { get; init; } = [];

    /// <summary>Only a move is worth undoing: a copy adds without removing or overwriting.
    /// A cancelled move still undoes — what got across is what goes back.</summary>
    public bool CanUndo => Verb == TransferVerb.Move && Completed.Count > 0;

    /// <summary>
    /// A copy whose writes can be taken back through <see cref="TransferExecutor.UndoCopies"/>.
    /// </summary>
    /// <remarks>
    /// Deliberately not folded into <see cref="CanUndo"/>, which answers "will
    /// <see cref="TransferExecutor.Undo"/> work" — and that refuses a copy outright. Nothing in Core
    /// ever sets this: it is an undertaking by the caller to keep the outcome, which is the exact
    /// condition <see cref="ConflictResolution.Overwrite"/> was written under. A caller that sets it
    /// without keeping the record would strand a displaced entry in staging for ever, which is the
    /// thing <see cref="CanUndo"/> being false for a copy has always been protecting against.
    /// </remarks>
    public bool CanUndoCopy { get; init; }
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
