namespace BertBrowser.Core.Services.Transfer;

/// <summary>A directory's recursive footprint, as the size index already knows it.</summary>
public readonly record struct DirectorySize(long Bytes, int Files);

/// <summary>
/// Where a byte total comes from. Deliberately an interface rather than a walk: this app's whole
/// premise is that <em>nothing scans the filesystem to size a folder</em> — the MFT pass has
/// already written a row for every directory on the volume, so the answer is a lookup.
/// </summary>
public interface ITransferSizeSource
{
    /// <summary>A directory's recursive size, or null when the index has no row for it — a
    /// non-NTFS volume, or one still being indexed.</summary>
    DirectorySize? Directory(string path);

    /// <summary>A file's size, or null when it cannot be read.</summary>
    long? File(string path);
}

/// <summary>
/// How much a plan will actually move, and whether that figure can be trusted.
/// </summary>
/// <param name="Bytes">Bytes that will be written.</param>
/// <param name="Files">Files that will be written.</param>
/// <param name="Complete">False when something in the plan had no size to hand. The figures are
/// then a floor, not a total, and must not be turned into a percentage or a time remaining —
/// showing "12%" of a number we do not know is worse than showing no number at all.</param>
public sealed record TransferEstimate(long Bytes, int Files, bool Complete)
{
    /// <summary>An empty plan, or one that is pure renaming: nothing to move, and we know it.</summary>
    public static TransferEstimate Nothing { get; } = new(0, 0, true);

    /// <summary>True when there is a real total to draw a bar against.</summary>
    public bool IsUsable => Complete && Bytes > 0;
}

/// <summary>
/// Works out what a <see cref="TransferPlan"/> costs in bytes, before it runs.
/// </summary>
public static class TransferEstimator
{
    public static TransferEstimate Estimate(TransferPlan plan, ITransferSizeSource sizes)
    {
        long bytes = 0;
        var files = 0;
        var complete = true;

        foreach (var transfer in plan.Transfers)
        {
            if (!MovesBytes(plan.Verb, transfer)) continue;

            if (transfer.IsDirectory)
            {
                if (sizes.Directory(transfer.SourcePath) is not { } size)
                {
                    complete = false;
                    continue;
                }
                bytes += size.Bytes;
                files += size.Files;
            }
            else
            {
                if (sizes.File(transfer.SourcePath) is not { } size)
                {
                    complete = false;
                    continue;
                }
                bytes += size;
                files++;
            }
        }

        return new TransferEstimate(bytes, files, complete);
    }

    /// <summary>
    /// Whether this item costs anything to transfer at all.
    /// </summary>
    /// <remarks>
    /// <b>A move within one volume is a rename.</b> It relocates 50 GB in the time it takes to
    /// update a directory entry, so counting those bytes would put a progress bar on screen that
    /// jumps from 0% to done — or worse, sits at 0% while eleven instant renames go past. Asks
    /// <see cref="TransferExecutor.SameVolume"/>, the same predicate the executor decides by, so
    /// the estimate and the work cannot disagree about what a rename is.
    /// </remarks>
    public static bool MovesBytes(TransferVerb verb, PlannedTransfer transfer) =>
        verb == TransferVerb.Copy ||
        !TransferExecutor.SameVolume(transfer.SourcePath, transfer.DestinationPath);
}
