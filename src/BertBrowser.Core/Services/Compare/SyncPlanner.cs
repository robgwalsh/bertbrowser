using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// Turning a comparison into what a one-way sync would do, and then into the plans the existing
/// executors already know how to carry out. Pure: it decides, and writes nothing.
/// </summary>
public static class SyncPlanner
{
    /// <summary>
    /// What making the right side match the left would take.
    /// </summary>
    /// <param name="removeRightOnly">Whether to offer deleting what only the right side has. Off
    /// is the safe default and the one the dialog opens with: everything else a sync does is
    /// additive or reversible in place, and this is the half that takes something away.</param>
    /// <remarks>
    /// <para>
    /// <b>A folder is acted on whole or not at all.</b> A folder the right side lacks becomes one
    /// action covering everything inside it, and nothing under it gets an action of its own —
    /// which is what lets <c>TransferExecutor</c> recurse into it as a single copy, and removes the
    /// question of who creates the intermediate folders. Nothing in this app has "make an empty
    /// folder nobody selected" as its job, and this is why it does not need to.
    /// </para>
    /// <para>
    /// An entry no verdict could be reached for produces no action at all, and is counted instead.
    /// You cannot sync what you could not compare, and quietly leaving it out of a list that reads
    /// as complete is how it would go unnoticed.
    /// </para>
    /// </remarks>
    public static SyncPreview Preview(FolderCompareOutcome compare, bool removeRightOnly)
    {
        ArgumentNullException.ThrowIfNull(compare);
        if (!compare.CanSync) return SyncPreview.Empty(compare.LeftPath, compare.RightPath);

        var result = compare.Result;
        var keys = result.ByRelativeKey.Keys.OrderBy(k => k, StringComparer.Ordinal).ToList();
        var leftBytes = SubtreeBytes(result.Left);
        var rightBytes = SubtreeBytes(result.Right);

        var actions = new List<SyncAction>();
        var covered = new List<string>();   // keys already acted on, whole
        var unknown = 0;

        foreach (var key in keys)
        {
            // A folder both sides have is opened, never acted on. Its verdict is the roll-up of
            // what is inside it, so treating that "differs" as something to write would replace a
            // whole tree to sync one file in it — and treating an "unknown" rolled up from one
            // descendant as an uncomparable entry would count the same doubt once per folder above
            // it. Only the entries that were really compared are counted or acted on.
            if (result.Left.TryGetValue(key, out var leftEntry) &&
                result.Right.TryGetValue(key, out var rightEntry) &&
                leftEntry.IsDirectory && rightEntry.IsDirectory)
                continue;

            var verdict = result.For(key);
            if (verdict is CompareVerdict.Unknown)
            {
                unknown++;
                continue;
            }

            // One action settles everything beneath the path it names, whichever kind it is: a copy
            // and an overwrite each write the whole of it, and a delete removes the whole of it.
            // Sorted ordinally, an ancestor is a strict prefix and so has already been seen. The
            // awkward case this covers is a file arriving where the right side keeps a folder —
            // the overwrite stages that folder away entire, so offering to delete what is inside it
            // as well would be an action against a path that will not be there.
            if (covered.Any(acted => CompareKeys.IsAtOrUnder(key, acted))) continue;

            if (CompareRules.WouldCopy(verdict))
            {
                var display = result.DisplayPath(key, CompareSide.Left);
                var entry = result.Left[key];
                var kind = result.Right.ContainsKey(key) ? SyncActionKind.Overwrite : SyncActionKind.Copy;

                actions.Add(new SyncAction(
                    key,
                    display,
                    kind,
                    Path.Combine(compare.LeftPath, display),
                    Path.Combine(compare.RightPath, display),
                    entry.IsDirectory,
                    Weigh(entry, key, leftBytes),
                    verdict,
                    // Overwriting a file the right side updated more recently is the one write a
                    // user would not expect to have agreed to by asking for a sync, so it is shown
                    // and left for them to tick.
                    Ticked: !CompareRules.OverwritesNewer(verdict)));

                covered.Add(key);
            }
            else if (removeRightOnly && CompareRules.WouldDelete(verdict))
            {
                var display = result.DisplayPath(key, CompareSide.Right);
                var entry = result.Right[key];

                actions.Add(new SyncAction(
                    key,
                    display,
                    SyncActionKind.Delete,
                    "",
                    Path.Combine(compare.RightPath, display),
                    entry.IsDirectory,
                    Weigh(entry, key, rightBytes),
                    verdict,
                    Ticked: true));

                covered.Add(key);
            }
        }

        return new SyncPreview(compare.LeftPath, compare.RightPath, actions, unknown);
    }

    /// <summary>Re-ticks a preview, so a dialog can hand back what the user changed without the
    /// planner having to know what a checkbox is.</summary>
    public static SyncPreview WithTicks(SyncPreview preview, IReadOnlySet<string> tickedKeys)
    {
        ArgumentNullException.ThrowIfNull(preview);
        ArgumentNullException.ThrowIfNull(tickedKeys);

        return preview with
        {
            Actions = [.. preview.Actions.Select(a => a with { Ticked = tickedKeys.Contains(a.RelativeKey) })],
        };
    }

    /// <summary>
    /// The ticked actions, as one transfer plan per destination folder plus one delete plan.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Every copy is given <see cref="ConflictResolution.Overwrite"/>, including the ones the
    /// comparison found a free name for. A sync says "make this side match", and a file that
    /// appeared on the right between the scan and the run is a name to take over rather than one
    /// to sidestep — <c>KeepBoth</c> would leave a silent "x (2)" behind and call the sync done.
    /// Nothing is lost by it: the displaced entry goes to staging and the whole run is undoable.
    /// </para>
    /// <para>
    /// Copies are grouped by destination folder and ordered parent-first. Every group's destination
    /// already exists — a folder the right side lacks was itself one action, so nothing under it
    /// reaches here — but the ordering costs nothing and is one less thing to be true by accident.
    /// </para>
    /// </remarks>
    public static SyncPlans ToPlans(
        SyncPreview preview, TransferPlanner transfers, DeletePlanner deletes, DeleteMode mode)
    {
        ArgumentNullException.ThrowIfNull(preview);
        ArgumentNullException.ThrowIfNull(transfers);
        ArgumentNullException.ThrowIfNull(deletes);

        var refused = new List<string>();
        var resolutions = new Dictionary<string, ConflictResolution>(StringComparer.Ordinal);
        var plans = new List<TransferPlan>();

        var copies = preview.Ticked.Where(a => a.Kind is not SyncActionKind.Delete);
        foreach (var group in copies
            .GroupBy(a => Path.GetDirectoryName(a.TargetPath) ?? "", StringComparer.OrdinalIgnoreCase)
            .OrderBy(g => g.Key.Length)
            .ThenBy(g => g.Key, StringComparer.Ordinal))
        {
            var plan = transfers.Plan(
                [.. group.Select(a => a.SourcePath)], group.Key, TransferVerb.Copy);

            foreach (var problem in plan.Problems)
                refused.Add(problem.Message);

            if (!plan.HasWork) continue;

            foreach (var transfer in plan.Transfers)
                resolutions[PathKey.Canonicalize(transfer.SourcePath)] = ConflictResolution.Overwrite;

            plans.Add(plan);
        }

        var removals = preview.Ticked.Where(a => a.Kind is SyncActionKind.Delete).ToList();
        var deletePlan = removals.Count == 0
            ? DeletePlan.Empty(mode)
            : deletes.Plan([.. removals.Select(a => new DeleteSource(a.TargetPath, a.IsDirectory))], mode);

        foreach (var problem in deletePlan.Problems)
            refused.Add(problem.Message);

        return new SyncPlans(plans, resolutions, deletePlan, refused);
    }

    /// <summary>
    /// What each folder's contents weigh, folded up the same way a verdict is.
    /// </summary>
    /// <remarks>
    /// The listings are already in hand, so this is exact rather than an estimate, and it is the
    /// reason a sync can show a byte total for a folder at all — <c>dir_size_cache</c> holds a
    /// number only for volumes the elevated pass has measured, which is precisely not the case for
    /// the backup drive on the other side of the comparison.
    /// </remarks>
    private static Dictionary<string, long> SubtreeBytes(IReadOnlyDictionary<string, CompareEntry> side)
    {
        var totals = new Dictionary<string, long>(StringComparer.Ordinal);
        foreach (var (key, entry) in side)
        {
            if (entry.IsDirectory) continue;
            foreach (var ancestor in CompareKeys.Ancestors(key))
                totals[ancestor] = totals.GetValueOrDefault(ancestor) + entry.SizeBytes;
        }
        return totals;
    }

    /// <summary>A file weighs what it says; a folder weighs its contents, or nothing when it has
    /// none. Never null in practice — a file with no measured length has no verdict either — but
    /// the type says so rather than the comment alone.</summary>
    private static long? Weigh(CompareEntry entry, string key, Dictionary<string, long> subtreeBytes) =>
        entry.IsDirectory ? subtreeBytes.GetValueOrDefault(key) : entry.SizeBytes;
}
