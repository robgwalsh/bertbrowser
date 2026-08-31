using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Elevation;

/// <summary>A transfer to run again with a token, and the failures it stands in for.</summary>
/// <param name="Covers">Canonical keys of the items in the first pass's failure list that this
/// retry answers. Carried rather than recomputed, because a merge otherwise has to guess which
/// original failure a result belongs to — and for a rename the two are not even the same path.</param>
public sealed record TransferRetry(
    TransferPlan Plan,
    IReadOnlyDictionary<string, ConflictResolution> Resolutions,
    IReadOnlyList<string> Covers);

/// <inheritdoc cref="TransferRetry"/>
public sealed record DeleteRetry(DeletePlan Plan, IReadOnlyList<string> Covers);

/// <inheritdoc cref="TransferRetry"/>
public sealed record RenameRetry(RenamePlan Plan, IReadOnlyList<string> Covers);

/// <inheritdoc cref="TransferRetry"/>
public sealed record NewItemRetry(NewItemPlan Plan);

/// <summary>The part of an undo to run again with a token, and the failures it stands in for.</summary>
public sealed record TransferUndoRetry(TransferOutcome Outcome, IReadOnlyList<string> Covers);

/// <inheritdoc cref="TransferUndoRetry"/>
public sealed record DeleteUndoRetry(DeleteOutcome Outcome, IReadOnlyList<string> Covers);

/// <summary>
/// Deriving the second, elevated pass from the first one's failures, and folding the two results
/// back into a single outcome.
/// </summary>
/// <remarks>
/// <para>
/// This is the whole of the feature's decision-making, and it is pure so that it can be held still
/// by a test. Everything else — the prompt, the pipe, the helper — is plumbing around it.
/// </para>
/// <para>
/// <b>A merged outcome must be indistinguishable from one the executor produced alone.</b> That is
/// what lets <c>RetireUndoable</c>, the one-level undo slot, <c>RefreshTabsShowingAsync</c> and the
/// tab fan-out carry on knowing nothing about elevation. It is also why the merge subtracts: an item
/// that was denied and then fixed must appear exactly once, in the completed list, and an item
/// denied twice exactly once, in the failures.
/// </para>
/// </remarks>
public static class ElevatedRetry
{
    // --- deriving the retry ---

    /// <summary>The transfer to run again with a token, or null when there is nothing a token could
    /// fix. A cancelled run is never retryable: a consent prompt in front of somebody who has just
    /// pressed Cancel is the wrong answer whatever else is true.</summary>
    public static TransferRetry? RetryFor(
        TransferPlan plan,
        TransferOutcome outcome,
        IReadOnlyDictionary<string, ConflictResolution>? resolutions = null)
    {
        if (outcome.Cancelled) return null;

        var covers = Denied(outcome.Failed, f => f.SourcePath, f => f.AccessDenied);
        if (covers.Count == 0) return null;

        var transfers = plan.Transfers
            .Where(t => covers.Contains(ElevationRules.KeyOf(t.SourcePath) ?? ""))
            .ToList();
        if (transfers.Count == 0) return null;

        var kept = new Dictionary<string, ConflictResolution>(StringComparer.Ordinal);
        if (resolutions is not null)
        {
            // The chosen resolutions travel with the retry. Without them a Replace silently becomes
            // the default KeepBoth and the operation quietly changes meaning half way through.
            foreach (var transfer in transfers)
            {
                var key = ElevationRules.KeyOf(transfer.SourcePath);
                if (key is not null && resolutions.TryGetValue(key, out var resolution))
                    kept[key] = resolution;
            }
        }

        // Rejected: [] on purpose. The planner's refusals are not permission problems, they were
        // reported once already, and carrying them would double them in the merged outcome.
        return new TransferRetry(
            new TransferPlan(plan.Verb, plan.DestinationDirectory, transfers, []), kept, [.. covers]);
    }

    /// <inheritdoc cref="RetryFor(TransferPlan, TransferOutcome, IReadOnlyDictionary{string, ConflictResolution})"/>
    public static DeleteRetry? RetryFor(DeletePlan plan, DeleteOutcome outcome)
    {
        var covers = Denied(outcome.Failed, f => f.SourcePath, f => f.AccessDenied);
        if (covers.Count == 0) return null;

        var deletions = plan.Deletions
            .Where(d => covers.Contains(ElevationRules.KeyOf(d.SourcePath) ?? ""))
            .ToList();
        if (deletions.Count == 0) return null;

        // The mode travels; the per-item disposition is re-derived against live disk by the executor
        // anyway, so an item that lost its Recycle Bin between the passes is still handled correctly.
        return new DeleteRetry(new DeletePlan(plan.Mode, deletions, []), [.. covers]);
    }

    /// <inheritdoc cref="RetryFor(TransferPlan, TransferOutcome, IReadOnlyDictionary{string, ConflictResolution})"/>
    public static RenameRetry? RetryFor(RenamePlan plan, RenameOutcome outcome)
    {
        var denied = outcome.Failed.Where(f => f.AccessDenied).ToList();
        if (denied.Count == 0) return null;

        // Where each denied item actually is. Usually its original path; a failed rename that could
        // not be put back leaves it parked under a staging name, and renaming from the path it no
        // longer occupies would fail and be reported as success.
        var actual = new Dictionary<string, string>(StringComparer.Ordinal);
        var covers = new List<string>();
        foreach (var failure in denied)
        {
            var key = ElevationRules.KeyOf(failure.SourcePath);
            if (key is null) continue;
            if (ElevationRules.IsRefusedForElevation(failure.StrandedPath ?? failure.SourcePath)) continue;
            actual[key] = failure.StrandedPath ?? failure.SourcePath;
            covers.Add(key);
        }
        if (covers.Count == 0) return null;

        var renames = new List<PlannedRename>();
        foreach (var item in plan.Renames)
        {
            var key = ElevationRules.KeyOf(item.SourcePath);
            if (key is null || !actual.TryGetValue(key, out var from)) continue;
            renames.Add(item with { SourcePath = from });
        }
        if (renames.Count == 0) return null;

        return new RenameRetry(new RenamePlan(renames, []), covers);
    }

    /// <inheritdoc cref="RetryFor(TransferPlan, TransferOutcome, IReadOnlyDictionary{string, ConflictResolution})"/>
    /// <remarks>There is only ever one item, which is why <c>NewItemPlan</c> carries one rejection
    /// rather than a list — so the retry is either the whole plan again or nothing.</remarks>
    public static NewItemRetry? RetryFor(NewItemPlan plan, NewItemOutcome outcome)
    {
        if (outcome.Failed is not { AccessDenied: true }) return null;
        if (!plan.HasWork) return null;

        // Deliberately no IsRefusedForElevation here. That rule guards a protected folder from being
        // moved, renamed or destroyed; creating a file inside one is the ordinary thing a file
        // browser does, and it is what Explorer does with the same prompt behind it.
        return new NewItemRetry(plan);
    }

    // --- putting things back ---

    /// <summary>
    /// The part of an undo to run again with a token, or null when there is nothing a token could
    /// fix.
    /// </summary>
    /// <remarks>
    /// <para>
    /// An undo takes an <em>outcome</em>, not a plan, so this narrows the outcome rather than a
    /// plan: the completions whose sources came back denied, and nothing else.
    /// </para>
    /// <para>
    /// <b><c>StagingDirectories</c> is deliberately emptied.</b> The unelevated half still holds
    /// items in its own staging folders, and the elevated pass's cleanup — which only removes an
    /// empty folder, but still — has no business running over them.
    /// </para>
    /// </remarks>
    public static TransferUndoRetry? UndoRetryFor(TransferOutcome outcome, TransferUndoResult result)
    {
        var covers = Denied(result.Failed, f => f.SourcePath, f => f.AccessDenied);
        if (covers.Count == 0) return null;

        var completed = outcome.Completed
            .Where(c => covers.Contains(ElevationRules.KeyOf(c.SourcePath) ?? ""))
            .ToList();
        if (completed.Count == 0) return null;

        return new TransferUndoRetry(
            outcome with { Completed = completed, Skipped = [], Failed = [], StagingDirectories = [] },
            [.. covers]);
    }

    /// <inheritdoc cref="UndoRetryFor(TransferOutcome, TransferUndoResult)"/>
    public static DeleteUndoRetry? UndoRetryFor(DeleteOutcome outcome, DeleteUndoResult result)
    {
        var covers = Denied(result.Failed, f => f.SourcePath, f => f.AccessDenied);
        if (covers.Count == 0) return null;

        var deleted = outcome.Deleted
            .Where(d => covers.Contains(ElevationRules.KeyOf(d.SourcePath) ?? ""))
            .ToList();
        if (deleted.Count == 0) return null;

        return new DeleteUndoRetry(
            outcome with { Deleted = deleted, Failed = [], StagingDirectories = [] },
            [.. covers]);
    }

    public static TransferUndoResult Merge(
        TransferUndoResult first, TransferUndoRetry retry, TransferUndoResult second) =>
        new(first.Restored + second.Restored,
            [.. Survivors(first.Failed, f => f.SourcePath, retry.Covers), .. second.Failed]);

    public static DeleteUndoResult Merge(
        DeleteUndoResult first, DeleteUndoRetry retry, DeleteUndoResult second) =>
        new(first.Restored + second.Restored,
            [.. Survivors(first.Failed, f => f.SourcePath, retry.Covers), .. second.Failed]);

    // --- folding the two results into one ---

    public static TransferOutcome Merge(TransferOutcome first, TransferRetry retry, TransferOutcome second)
    {
        RequireSameShape(first.Verb == second.Verb, "verb");
        RequireSameShape(
            string.Equals(
                ElevationRules.KeyOf(first.DestinationDirectory),
                ElevationRules.KeyOf(second.DestinationDirectory),
                StringComparison.Ordinal),
            "destination");

        return new TransferOutcome(
            first.Verb,
            first.DestinationDirectory,
            [.. first.Completed, .. second.Completed],
            [.. first.Skipped, .. second.Skipped],
            [.. Survivors(first.Failed, f => f.SourcePath, retry.Covers), .. second.Failed],
            [.. first.StagingDirectories, .. second.StagingDirectories],
            first.Cancelled || second.Cancelled);
    }

    public static DeleteOutcome Merge(DeleteOutcome first, DeleteRetry retry, DeleteOutcome second)
    {
        RequireSameShape(first.Permanent == second.Permanent, "delete mode");

        return new DeleteOutcome(
            first.Permanent,
            [.. first.Deleted, .. second.Deleted],
            [.. Survivors(first.Failed, f => f.SourcePath, retry.Covers), .. second.Failed],
            [.. first.StagingDirectories, .. second.StagingDirectories]);
    }

    public static RenameOutcome Merge(RenameOutcome first, RenameRetry retry, RenameOutcome second)
    {
        // The retry renamed from wherever the item actually was, which for a stranded item is not
        // the path the user knows it by. The completed records are rewritten back onto the original
        // sources, so undo — which is this same execution with every path swapped — puts things back
        // where they came from rather than into a staging name.
        var origins = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var item in first.Failed)
        {
            if (item.StrandedPath is not { } stranded) continue;
            var key = ElevationRules.KeyOf(stranded);
            if (key is not null) origins[key] = item.SourcePath;
        }

        var completed = second.Completed.Select(c =>
            ElevationRules.KeyOf(c.SourcePath) is { } key && origins.TryGetValue(key, out var origin)
                ? c with { SourcePath = origin }
                : c);

        return new RenameOutcome(
            [.. first.Completed, .. completed],
            [.. Survivors(first.Failed, f => f.SourcePath, retry.Covers), .. second.Failed]);
    }

    public static NewItemOutcome Merge(NewItemOutcome first, NewItemRetry retry, NewItemOutcome second)
    {
        _ = first;
        _ = retry;
        // One item, so there is nothing to fold: the second attempt is the answer, whichever way it
        // went. Kept as a Merge for symmetry with the other three, so a caller written from one of
        // them reads correctly here.
        return second;
    }

    // --- shared ---

    /// <summary>Canonical keys of the failures a token could plausibly fix.</summary>
    private static HashSet<string> Denied<T>(
        IEnumerable<T> failures, Func<T, string> path, Func<T, bool> accessDenied)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var failure in failures)
        {
            if (!accessDenied(failure)) continue;
            if (ElevationRules.IsRefusedForElevation(path(failure))) continue;
            if (ElevationRules.KeyOf(path(failure)) is { } key) keys.Add(key);
        }
        return keys;
    }

    /// <summary>The first pass's failures that the retry did not answer for.</summary>
    private static IEnumerable<T> Survivors<T>(
        IEnumerable<T> failed, Func<T, string> path, IReadOnlyList<string> covers)
    {
        var covered = new HashSet<string>(covers, StringComparer.Ordinal);
        return failed.Where(f => ElevationRules.KeyOf(path(f)) is not { } key || !covered.Contains(key));
    }

    private static void RequireSameShape(bool same, string what)
    {
        if (!same)
            throw new ArgumentException($"The two passes disagree about the {what} of the operation.");
    }
}
