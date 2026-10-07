namespace BertBrowser.Core.Services.Timestamps;

/// <summary>
/// Sets the modified and created dates of files and folders, and puts them back.
/// </summary>
/// <remarks>
/// <para>
/// Nothing is staged: a date is two numbers, and the outcome carries the old ones. Undo and redo
/// are one operation, <see cref="Revert"/>, which sets an entry back to its <em>before</em> and
/// returns it with the two swapped.
/// </para>
/// <para>
/// One item's failure never affects the others, and a revert leaves alone anything whose dates
/// have moved since — something else has written to it, and its new date is true.
/// </para>
/// </remarks>
public sealed class TimestampExecutor
{
    private readonly ITimestampProbe _probe;

    public TimestampExecutor(ITimestampProbe probe) => _probe = probe;

    public TimestampExecutor() : this(new FileSystemTimestampProbe())
    {
    }

    public TimestampOutcome Execute(TimestampPlan plan)
    {
        if (!plan.HasWork) return TimestampOutcome.Empty;

        var stamped = new List<StampedEntry>();
        var failed = new List<FailedTimestamp>();

        foreach (var item in plan.Items)
        {
            var name = Path.GetFileName(item.Path);
            try
            {
                // The planner saw an entry here; disk is the authority on whether it still is one.
                if (_probe.Find(item.Path) is not { } before)
                {
                    failed.Add(new FailedTimestamp(item.Path, $"{name} is no longer there."));
                    continue;
                }

                if ((before.Attributes & FileAttributes.ReparsePoint) != 0)
                {
                    failed.Add(new FailedTimestamp(item.Path, $"{name} is a link, and its date is its target's."));
                    continue;
                }

                var directory = (before.Attributes & FileAttributes.Directory) != 0;
                Set(item.Path, directory, item.ModifiedUtc, item.CreatedUtc);

                var after = _probe.Find(item.Path) ?? before;
                stamped.Add(new StampedEntry(
                    item.Path, directory,
                    before.ModifiedUtc, before.CreatedUtc, after.ModifiedUtc, after.CreatedUtc));
            }
            catch (Exception ex) when (IsFailure(ex))
            {
                failed.Add(new FailedTimestamp(
                    item.Path, $"The date of {name} could not be changed: {ex.Message}", AccessDenied.Caused(ex)));
            }
        }

        return new TimestampOutcome(stamped, failed);
    }

    /// <summary>
    /// Sets each entry back to the dates it had, and returns the ones that moved with their before
    /// and after exchanged — so reverting the result is the redo.
    /// </summary>
    public TimestampOutcome Revert(IReadOnlyList<StampedEntry> stamped)
    {
        var reverted = new List<StampedEntry>();
        var failed = new List<FailedTimestamp>();

        foreach (var entry in stamped)
        {
            var name = Path.GetFileName(entry.Path);
            try
            {
                if (_probe.Find(entry.Path) is not { } now)
                {
                    failed.Add(new FailedTimestamp(entry.Path, $"{name} is no longer there."));
                    continue;
                }

                if (now.ModifiedUtc != entry.AfterModifiedUtc || now.CreatedUtc != entry.AfterCreatedUtc)
                {
                    failed.Add(new FailedTimestamp(
                        entry.Path, $"{name} has changed since, so its date was left as it is."));
                    continue;
                }

                Set(entry.Path, entry.IsDirectory, entry.BeforeModifiedUtc, entry.BeforeCreatedUtc);

                var after = _probe.Find(entry.Path) ?? now;
                reverted.Add(new StampedEntry(
                    entry.Path, entry.IsDirectory,
                    now.ModifiedUtc, now.CreatedUtc, after.ModifiedUtc, after.CreatedUtc));
            }
            catch (Exception ex) when (IsFailure(ex))
            {
                failed.Add(new FailedTimestamp(
                    entry.Path, $"The date of {name} could not be put back: {ex.Message}", AccessDenied.Caused(ex)));
            }
        }

        return new TimestampOutcome(reverted, failed);
    }

    private static void Set(string path, bool directory, DateTime? modifiedUtc, DateTime? createdUtc)
    {
        // Created first: on some file systems setting it nudges the last-write time.
        if (createdUtc is { } created)
        {
            if (directory) Directory.SetCreationTimeUtc(path, created);
            else File.SetCreationTimeUtc(path, created);
        }

        if (modifiedUtc is { } modified)
        {
            if (directory) Directory.SetLastWriteTimeUtc(path, modified);
            else File.SetLastWriteTimeUtc(path, modified);
        }
    }

    private static bool IsFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException;
}
