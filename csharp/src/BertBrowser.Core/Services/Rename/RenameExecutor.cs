using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Rename;

/// <summary>
/// Carries out a <see cref="RenamePlan"/>. Nothing is ever overwritten: every move goes through the
/// non-replacing overloads, and a name that turns out to be taken fails that one item and leaves the
/// rest alone.
/// </summary>
/// <remarks>
/// The awkward part is a batch whose names overlap — rotating "a, b" into "b, a", or shifting a
/// numbered set along by one. Anything whose current name <em>another</em> item is aiming at is
/// moved aside to a temporary name first, so the second pass only ever writes into empty space.
/// A failure in that second pass puts the item back under its original name; if even that fails,
/// the staged path is named in the error rather than being left silently.
/// </remarks>
public sealed class RenameExecutor
{
    public RenameOutcome Execute(RenamePlan plan)
    {
        var work = plan.Work;
        if (work.Count == 0) return RenameOutcome.Empty;

        var completed = new List<CompletedRename>();
        var failed = new List<FailedRename>();

        // Which item, if any, is aiming at each name. Targets are unique within a plan, so this is
        // one-to-one, and an item that only changes its own casing maps to itself — which is not
        // something it has to wait for.
        var wantedBy = new Dictionary<string, int>(StringComparer.Ordinal);
        for (var i = 0; i < work.Count; i++)
            wantedBy[PathKey.Canonicalize(work[i].TargetPath)] = i;

        // Where each item sits now — the staging pass moves some of them out from under themselves —
        // and which of them are standing on a temporary name as a result. Indexed, not keyed by the
        // item, because a plan is a list of records and records compare by value.
        var current = work.Select(w => w.SourcePath).ToArray();
        var isStaged = new bool[work.Count];
        var stumbled = new bool[work.Count];

        for (var i = 0; i < work.Count; i++)
        {
            var item = work[i];
            if (!wantedBy.TryGetValue(PathKey.Canonicalize(item.SourcePath), out var wanter) ||
                wanter == i) continue;

            try
            {
                var temporary = StagingPath(item);
                Move(item.SourcePath, temporary, item.IsDirectory);
                current[i] = temporary;
                isStaged[i] = true;
            }
            catch (Exception ex) when (IsFilesystemFailure(ex))
            {
                stumbled[i] = true;
                failed.Add(new FailedRename(
                    item.SourcePath,
                    $"'{item.SourceName}' could not be renamed: {ex.Message}",
                    AccessDenied.Caused(ex)));
            }
        }

        for (var i = 0; i < work.Count; i++)
        {
            if (stumbled[i]) continue;

            var item = work[i];
            var from = current[i];
            try
            {
                // The planner decided this name was free; disk is the authority, and it may have
                // changed while the dialog was open.
                if (!string.Equals(PathKey.Canonicalize(from), PathKey.Canonicalize(item.TargetPath),
                        StringComparison.Ordinal) && Exists(item.TargetPath))
                    throw new IOException($"'{item.TargetName}' already exists.");

                Move(from, item.TargetPath, item.IsDirectory);
                completed.Add(new CompletedRename(item.SourcePath, item.TargetPath, item.IsDirectory));
            }
            catch (Exception ex) when (IsFilesystemFailure(ex))
            {
                // Where the item actually ended up, which is only ever somewhere unexpected when
                // putting it back failed too. Taken as a value rather than spliced into the message,
                // so a retry can start from it — see FailedRename.StrandedPath.
                var stranded = Restore(item, from, isStaged[i]);
                var note = stranded is null ? "" : $" It is currently at '{stranded}'.";
                failed.Add(new FailedRename(
                    item.SourcePath,
                    $"'{item.SourceName}' could not be renamed: {ex.Message}{note}",
                    AccessDenied.Caused(ex),
                    stranded));
            }
        }

        return new RenameOutcome(completed, failed);
    }

    // --- Undo ---

    /// <summary>
    /// Puts a completed rename back. A rename is its own inverse, so this is the same execution with
    /// every path swapped — which means undoing a rotation, or a batch that shifted a numbered set
    /// along, goes through exactly the staging the forward direction did. An item whose old name has
    /// since been taken is reported rather than overwritten, and one that has been renamed again in
    /// the meantime simply fails: nothing here writes over anything.
    /// </summary>
    public RenameOutcome Undo(RenameOutcome outcome) => Execute(UndoPlan(outcome));

    /// <summary>The plan that reverses <paramref name="outcome"/>.</summary>
    public static RenamePlan UndoPlan(RenameOutcome outcome) =>
        new(outcome.Completed
            .Select(c => new PlannedRename(c.FinalPath, c.SourcePath, c.IsDirectory))
            .ToList(), []);

    /// <summary>Puts a staged item back under its original name after its rename failed. Returns
    /// null when there was nothing to put back or it went back cleanly, and the staged path when it
    /// could not — the one case where an item is left somewhere the user didn't put it, so it must
    /// never be silent, and the one case where a retry has to start from somewhere other than the
    /// path the plan named.</summary>
    private static string? Restore(PlannedRename item, string from, bool wasStaged)
    {
        if (!wasStaged) return null;
        try
        {
            Move(from, item.SourcePath, item.IsDirectory);
            return null;
        }
        catch (Exception ex) when (IsFilesystemFailure(ex))
        {
            return from;
        }
    }

    /// <summary>A name nothing else can be sitting on, in the item's own folder so the move stays a
    /// rename rather than becoming a cross-volume copy.</summary>
    private static string StagingPath(PlannedRename item)
    {
        var directory = Path.GetDirectoryName(item.SourcePath)
            ?? throw new IOException($"'{item.SourceName}' has no parent folder.");
        return Path.Combine(directory, $".bertbrowser-rename-{Guid.NewGuid():N}");
    }

    private static void Move(string from, string to, bool isDirectory)
    {
        if (isDirectory) Directory.Move(from, to);
        else File.Move(from, to, overwrite: false);
    }

    private static bool Exists(string path) => Directory.Exists(path) || File.Exists(path);

    private static bool IsFilesystemFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
