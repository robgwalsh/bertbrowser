namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>Sizes of folders the index already knows, by their original path — one primary-key
/// lookup, the same kind of answer a transfer's estimate takes.</summary>
public interface IHeldFolderSizes
{
    /// <summary>The folder's recursive size, or null when the index has no complete answer.</summary>
    long? Of(string originalPath);
}

/// <summary>
/// How much a record holds, for the history's byte budget.
/// </summary>
/// <remarks>
/// <para>
/// A file is one stat, and exact. A folder comes from the size index first, by the path it had
/// before it was set aside; when the index does not know it, the held copy itself is walked — it is
/// this app's own hidden folder, so nothing else is racing the walk — but only so far. The walk has
/// an entry cap and a time cap, and a walk that hits either says so, so the total is a floor rather
/// than a guess.
/// </para>
/// <para>
/// This only ever decides when an old entry is released, never what anybody is shown as a folder's
/// size, which is why a walk is acceptable here where it is not in a listing.
/// </para>
/// </remarks>
public sealed class HeldSizeEstimator(IHeldFolderSizes? index = null)
{
    public const int WalkEntryCap = 50_000;

    public static readonly TimeSpan WalkTimeCap = TimeSpan.FromSeconds(1);

    public HeldSize Measure(IUndoableRecord record) =>
        record.Held.Aggregate(HeldSize.None, (sum, held) => sum + Measure(held));

    public HeldSize Measure(HeldEntry held)
    {
        try
        {
            if (!held.IsDirectory)
            {
                var file = new FileInfo(held.HeldPath);
                return file.Exists ? new HeldSize(file.Length, true) : HeldSize.None;
            }

            if (index?.Of(held.OriginalPath) is { } known) return new HeldSize(known, true);
            return Walk(held.HeldPath);
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return new HeldSize(0, false);
        }
    }

    /// <summary>A bounded, link-safe walk: a junction inside a held folder is counted as itself,
    /// never followed into whatever it points at.</summary>
    private static HeldSize Walk(string root)
    {
        var start = new DirectoryInfo(root);
        if (!start.Exists) return HeldSize.None;

        var deadline = DateTime.UtcNow + WalkTimeCap;
        var bytes = 0L;
        var seen = 0;
        var complete = true;
        var pending = new Stack<DirectoryInfo>();
        pending.Push(start);

        while (pending.Count > 0)
        {
            if (seen >= WalkEntryCap || DateTime.UtcNow > deadline) return new HeldSize(bytes, false);

            var current = pending.Pop();
            try
            {
                foreach (var entry in current.EnumerateFileSystemInfos())
                {
                    seen++;
                    if (DirectoryRemoval.IsLink(entry)) continue;
                    if (entry is FileInfo file) bytes += file.Length;
                    else if (entry is DirectoryInfo child) pending.Push(child);
                }
            }
            catch (Exception ex) when (IsReadFailure(ex))
            {
                complete = false;
            }
        }

        return new HeldSize(bytes, complete);
    }

    private static bool IsReadFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or ArgumentException or NotSupportedException;
}
