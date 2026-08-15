namespace BertBrowser.Core.Services.Delete;

/// <summary>One item the Recycle Bin took.</summary>
/// <param name="SourcePath">Where it was.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="RecycledPath">Its <c>$R</c> path inside the bin, or null when the shell erased it
/// outright instead of holding it — which is not a failure, but does mean there is nothing to
/// restore.</param>
public sealed record RecycledItem(string SourcePath, bool IsDirectory, string? RecycledPath);

/// <param name="Recycled">Items the bin accepted.</param>
/// <param name="Failed">Items it refused, one message each. A failure here never affects the rest.</param>
public sealed record RecycleResult(
    IReadOnlyList<RecycledItem> Recycled, IReadOnlyList<FailedDelete> Failed);

/// <summary>
/// The Windows Recycle Bin, as much of it as this app needs: put items in, and take one back out.
/// </summary>
/// <remarks>
/// <para>
/// Implemented in the App project over <c>IFileOperation</c>, because Core carries no shell
/// dependency — and because that keeps the executor's own rules testable against an in-memory bin
/// rather than against the user's real one.
/// </para>
/// <para>
/// <see cref="Recycle"/> is a batch: the shell wants one operation with every item added to it, and
/// that is also the only way to get a single progress sink and a single confirmation.
/// </para>
/// </remarks>
public interface IRecycleBin
{
    /// <summary>Sends items to the bin. Never throws for an individual item — a refusal comes back
    /// in <see cref="RecycleResult.Failed"/> so one bad item cannot cost the others.</summary>
    RecycleResult Recycle(
        IReadOnlyList<PlannedDelete> items,
        CancellationToken ct = default,
        IProgress<DeleteProgress>? progress = null);

    /// <summary>Puts one item back where it came from. False when the bin no longer holds it — it
    /// was emptied, swept by Storage Sense, or restored by hand in the meantime.</summary>
    bool Restore(DeletedItem item);
}
