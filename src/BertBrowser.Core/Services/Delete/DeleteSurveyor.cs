namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// Measures what a <see cref="DeletePlan"/> would actually remove, so the confirmation can say
/// "3 folders, 1,204 files, 3.2 GB" rather than "3 items". Nothing here writes, and nothing here
/// is required: a delete whose survey was cancelled still deletes exactly the same items.
/// </summary>
/// <remarks>
/// The walk skips reparse points instead of following them — a junction is counted as the one
/// directory entry it is, because that is all deleting it removes, and following it would report
/// (and appear to threaten) a tree that lives somewhere else entirely. A folder that cannot be read
/// marks the measurement <see cref="DeleteMeasurement.Incomplete"/> rather than failing, so the
/// totals are honest about being a floor.
/// </remarks>
public sealed class DeleteSurveyor
{
    /// <param name="progress">Reports each item as it finishes, so a dialog can fill in row by row
    /// instead of waiting for a large tree at the end of the list.</param>
    public DeleteSurvey Survey(
        DeletePlan plan, CancellationToken ct = default, IProgress<DeleteMeasurement>? progress = null)
    {
        var items = new List<DeleteMeasurement>();
        foreach (var item in plan.Deletions)
        {
            if (ct.IsCancellationRequested) break;
            var measurement = Measure(item, ct);
            items.Add(measurement);
            progress?.Report(measurement);
        }
        return new DeleteSurvey(items);
    }

    /// <summary>Measures one planned item. Never throws: an unreadable item is reported as empty
    /// and incomplete.</summary>
    public static DeleteMeasurement Measure(PlannedDelete item, CancellationToken ct = default)
    {
        if (!item.IsDirectory)
        {
            try
            {
                var info = new FileInfo(item.SourcePath);
                return new DeleteMeasurement(item.SourcePath, false, info.Exists ? info.Length : 0,
                    info.Exists ? 1 : 0, 0, !info.Exists);
            }
            catch (Exception ex) when (IsWalkFailure(ex))
            {
                return new DeleteMeasurement(item.SourcePath, false, 0, 1, 0, true);
            }
        }

        var bytes = 0L;
        var files = 0;
        var directories = 1; // the folder itself goes too
        var incomplete = false;

        var pending = new Stack<DirectoryInfo>();
        try
        {
            var root = new DirectoryInfo(item.SourcePath);
            if (!root.Exists) return new DeleteMeasurement(item.SourcePath, true, 0, 0, 1, true);
            if (IsLink(root)) return new DeleteMeasurement(item.SourcePath, true, 0, 0, 1, false);
            pending.Push(root);
        }
        catch (Exception ex) when (IsWalkFailure(ex))
        {
            return new DeleteMeasurement(item.SourcePath, true, 0, 0, 1, true);
        }

        while (pending.Count > 0)
        {
            if (ct.IsCancellationRequested)
            {
                incomplete = true;
                break;
            }

            var current = pending.Pop();
            FileSystemInfo[] entries;
            try
            {
                entries = current.GetFileSystemInfos();
            }
            catch (Exception ex) when (IsWalkFailure(ex))
            {
                incomplete = true;
                continue;
            }

            foreach (var entry in entries)
            {
                try
                {
                    if (entry is DirectoryInfo child)
                    {
                        directories++;
                        // A junction is one entry to delete, not the tree it points at.
                        if (!IsLink(child)) pending.Push(child);
                    }
                    else if (entry is FileInfo file)
                    {
                        files++;
                        if (!IsLink(file)) bytes += file.Length;
                    }
                }
                catch (Exception ex) when (IsWalkFailure(ex))
                {
                    incomplete = true;
                }
            }
        }

        return new DeleteMeasurement(item.SourcePath, true, bytes, files, directories, incomplete);
    }

    private static bool IsLink(FileSystemInfo info) =>
        (info.Attributes & FileAttributes.ReparsePoint) != 0;

    private static bool IsWalkFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
