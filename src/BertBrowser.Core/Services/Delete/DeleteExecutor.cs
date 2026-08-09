using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// Carries out a <see cref="DeletePlan"/>.
/// </summary>
/// <remarks>
/// <para>
/// An ordinary delete does not erase anything. Each item is <em>moved</em> into a hidden holding
/// folder at the root of its own volume — a rename, so it costs the same for one file as for a
/// hundred gigabytes — and stays there, intact, until the delete can no longer be undone. That is
/// what makes Ctrl+Z able to put a directory tree back byte for byte instead of best-effort, and it
/// is the same trick a Replace already uses to displace an entry it takes over from.
/// <see cref="CommitStaging"/> is what finally erases the data, and the caller owes it exactly one
/// call per outcome once the undo record is retired.
/// </para>
/// <para>
/// A permanent delete (Shift+Delete) skips all of that and erases in place. It is the only path
/// here that destroys data, so it is the only one that cannot be undone, and it is never taken
/// unless the plan says so.
/// </para>
/// <para>
/// Every rule the planner applied is re-checked against live disk state, because a plan is built
/// while the confirmation is open and carried out after it is answered. A failure on one item never
/// affects the others.
/// </para>
/// </remarks>
public sealed class DeleteExecutor
{
    /// <summary>ERROR_NOT_SAME_DEVICE as an HRESULT. A volume-root holding folder is on the same
    /// volume as the item nearly always — but a mount point part-way down the path makes "the root
    /// of C:" and "where this file physically lives" two different volumes, and then the move has
    /// to be staged locally instead.</summary>
    private const int HResultNotSameDevice = unchecked((int)0x80070011);

    /// <summary>The per-volume holding folder, at the volume root so it is one place per disk and
    /// can be swept up later even after a crash.</summary>
    internal const string TrashFolderName = ".bertbrowser-trash";

    /// <summary>One folder per delete inside <see cref="TrashFolderName"/>.</summary>
    internal const string BatchPrefix = "delete-";

    /// <summary>The fallback holding folder, made beside the item itself, which is guaranteed to be
    /// on the item's own volume whatever mount points the path crosses.</summary>
    internal const string LocalStagingPrefix = ".bertbrowser-deleted-";

    private readonly IDeleteProbe _probe;
    private readonly IReadOnlyCollection<string> _protectedKeys;
    private readonly string? _stagingRoot;

    /// <param name="probe">How the executor asks what is on disk.</param>
    /// <param name="protectedPaths">Locations to refuse outright; defaults to
    /// <see cref="ProtectedLocations.Default"/>.</param>
    /// <param name="stagingRoot">Where to put the holding folder, instead of the root of each
    /// item's own volume. Only for tests, which have no business creating folders at the root of
    /// the machine's disks — and, being under the temp directory, still land on the same volume as
    /// the files they make, which is what the whole design rests on.</param>
    public DeleteExecutor(
        IDeleteProbe probe, IEnumerable<string>? protectedPaths = null, string? stagingRoot = null)
    {
        _probe = probe;
        _protectedKeys = protectedPaths is null
            ? ProtectedLocations.Default
            : ProtectedLocations.KeysOf(protectedPaths);
        _stagingRoot = stagingRoot;
    }

    public DeleteExecutor() : this(new FileSystemDeleteProbe())
    {
    }

    public DeleteOutcome Execute(
        DeletePlan plan, CancellationToken ct = default, IProgress<DeleteProgress>? progress = null)
    {
        var deleted = new List<DeletedItem>();
        var failed = new List<FailedDelete>();
        var staging = new List<string>();

        // One holding folder per volume, and one per parent folder for the mount-point fallback,
        // both made on first use so a plan that fails immediately leaves nothing behind.
        var volumeBatches = new Dictionary<string, string>(StringComparer.Ordinal);
        var localBatches = new Dictionary<string, string>(StringComparer.Ordinal);

        var done = 0;
        foreach (var item in plan.Deletions)
        {
            if (ct.IsCancellationRequested) break;
            progress?.Report(new DeleteProgress(done, plan.Deletions.Count, item.Name));
            done++;

            try
            {
                Revalidate(item);

                if (plan.Permanent)
                {
                    Erase(item.SourcePath, item.IsDirectory);
                    deleted.Add(new DeletedItem(item.SourcePath, item.IsDirectory, null));
                }
                else
                {
                    var staged = Stage(item, volumeBatches, localBatches, staging);
                    deleted.Add(new DeletedItem(item.SourcePath, item.IsDirectory, staged));
                }
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                failed.Add(new FailedDelete(item.SourcePath, $"{item.Name}: {ex.Message}"));
            }
        }

        progress?.Report(new DeleteProgress(done, plan.Deletions.Count, ""));
        return new DeleteOutcome(plan.Permanent, deleted, failed, staging);
    }

    /// <summary>Re-applies the planner's rules against live disk state.</summary>
    private void Revalidate(PlannedDelete item)
    {
        if (!Exists(item.SourcePath))
            throw new FileNotFoundException($"'{item.Name}' no longer exists.", item.SourcePath);

        var full = Path.TrimEndingDirectorySeparator(Path.GetFullPath(item.SourcePath));
        if (string.IsNullOrEmpty(Path.GetDirectoryName(full)))
            throw new IOException($"'{item.SourcePath}' is a drive root and cannot be deleted.");

        if (_protectedKeys.Contains(PathKey.Canonicalize(item.SourcePath)))
            throw new IOException($"'{item.SourcePath}' is a system location and will not be deleted.");
    }

    // --- Staged delete ---

    /// <summary>Moves one item into a holding folder and returns where it now sits.</summary>
    private string Stage(
        PlannedDelete item,
        Dictionary<string, string> volumeBatches,
        Dictionary<string, string> localBatches,
        List<string> staging)
    {
        var parent = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(item.SourcePath)))
            ?? throw new IOException($"'{item.Name}' has no parent folder.");

        if (VolumeBatch(item, volumeBatches, staging) is { } batch &&
            TryMoveInto(item, batch, out var staged))
            return staged;

        // Either the volume root refused the folder, or the item turned out to live on a different
        // volume than its path root suggests. A folder beside the item cannot be either.
        var localKey = PathKey.Canonicalize(parent);
        if (!localBatches.TryGetValue(localKey, out var local))
        {
            local = CreateStagingDirectory(Path.Combine(parent, LocalStagingPrefix + ShortId()));
            localBatches[localKey] = local;
            staging.Add(local);
        }
        return MoveInto(item, local);
    }

    /// <summary>The holding folder at the root of the item's volume, made on first use. Null when
    /// the root will not take one — a read-only or otherwise unwritable root, or an item that
    /// <em>is</em> the holding folder.</summary>
    private string? VolumeBatch(
        PlannedDelete item, Dictionary<string, string> volumeBatches, List<string> staging)
    {
        string? root;
        try
        {
            root = _stagingRoot ?? Path.GetPathRoot(Path.GetFullPath(item.SourcePath));
        }
        catch (Exception ex) when (IsDeleteFailure(ex))
        {
            return null;
        }
        if (string.IsNullOrEmpty(root)) return null;

        var rootKey = PathKey.Canonicalize(root);
        if (!volumeBatches.TryGetValue(rootKey, out var batch))
        {
            try
            {
                var trash = CreateStagingDirectory(Path.Combine(root, TrashFolderName));
                batch = CreateStagingDirectory(Path.Combine(trash, BatchPrefix + ShortId()));
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                return null;
            }
            volumeBatches[rootKey] = batch;
            staging.Add(batch);
        }

        // Deleting the holding folder itself, or anything above it, would have it move into itself.
        var sourceKey = PathKey.Canonicalize(item.SourcePath);
        var batchKey = PathKey.Canonicalize(batch);
        return batchKey == sourceKey || PathKey.IsUnder(batchKey, sourceKey) ? null : batch;
    }

    /// <summary>Moves the item into <paramref name="directory"/>. False — with nothing moved — when
    /// the item turns out to be on another volume; every other failure is a real one and throws.</summary>
    private bool TryMoveInto(PlannedDelete item, string directory, out string staged)
    {
        staged = "";
        try
        {
            staged = MoveInto(item, directory);
            return true;
        }
        catch (IOException ex) when (ex.HResult == HResultNotSameDevice)
        {
            return false;
        }
    }

    private string MoveInto(PlannedDelete item, string directory)
    {
        // Two selected items can share a name — a flattened search result spans folders — so the
        // holding folder numbers the second one rather than colliding with the first.
        var staged = UniquePath(Path.Combine(directory, item.Name));
        MoveEntry(item.SourcePath, staged, item.IsDirectory);
        return staged;
    }

    private static void MoveEntry(string from, string to, bool isDirectory)
    {
        if (isDirectory) Directory.Move(from, to);
        else File.Move(from, to, overwrite: false);
    }

    // --- Permanent delete ---

    /// <summary>
    /// Erases in place. A read-only item is retried after clearing the attribute — the user asked
    /// for this one by name, and a tree that stops half-deleted on the first read-only file inside
    /// it is worse than one that goes.
    /// </summary>
    private static void Erase(string path, bool isDirectory)
    {
        try
        {
            Remove(path, isDirectory);
        }
        catch (UnauthorizedAccessException)
        {
            ClearReadOnly(path, isDirectory);
            Remove(path, isDirectory);
        }
    }

    private static void Remove(string path, bool isDirectory)
    {
        if (isDirectory) Directory.Delete(path, recursive: true);
        else File.Delete(path);
    }

    /// <summary>Best-effort: anything still read-only afterwards fails the retry, which is reported.</summary>
    private static void ClearReadOnly(string path, bool isDirectory)
    {
        try
        {
            if (!isDirectory)
            {
                File.SetAttributes(path, File.GetAttributes(path) & ~FileAttributes.ReadOnly);
                return;
            }

            var root = new DirectoryInfo(path);
            foreach (var entry in root.EnumerateFileSystemInfos("*", SearchOption.AllDirectories))
            {
                if ((entry.Attributes & FileAttributes.ReparsePoint) != 0) continue;
                if ((entry.Attributes & FileAttributes.ReadOnly) != 0)
                    entry.Attributes &= ~FileAttributes.ReadOnly;
            }
            if ((root.Attributes & FileAttributes.ReadOnly) != 0)
                root.Attributes &= ~FileAttributes.ReadOnly;
        }
        catch (Exception ex) when (IsDeleteFailure(ex))
        {
        }
    }

    // --- Undo ---

    /// <summary>
    /// Puts a staged delete back: each item returns from the holding folder to exactly where it
    /// was. An item whose original path has since been taken is reported rather than overwritten —
    /// nothing here writes over anything.
    /// </summary>
    public DeleteUndoResult Undo(DeleteOutcome outcome, CancellationToken ct = default)
    {
        if (outcome.Permanent)
            return new DeleteUndoResult(0, [new FailedDelete("", "A permanent delete cannot be undone.")]);

        var failed = new List<FailedDelete>();
        var restored = 0;

        // Reverse order, so a batch unwinds in the order it was made.
        foreach (var item in outcome.Deleted.Reverse())
        {
            if (ct.IsCancellationRequested) break;
            try
            {
                if (item.StagedPath is not { } staged || !Exists(staged))
                {
                    failed.Add(new FailedDelete(item.SourcePath,
                        $"{item.Name}: the deleted copy is no longer being held."));
                    continue;
                }
                if (Exists(item.SourcePath))
                {
                    failed.Add(new FailedDelete(item.SourcePath,
                        $"{item.Name}: something else now occupies its original location."));
                    continue;
                }

                var parent = Path.GetDirectoryName(item.SourcePath);
                if (parent is null || !_probe.DirectoryExists(parent))
                {
                    failed.Add(new FailedDelete(item.SourcePath, $"{item.Name}: its original folder is gone."));
                    continue;
                }

                MoveEntry(staged, item.SourcePath, item.IsDirectory);
                restored++;
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                failed.Add(new FailedDelete(item.SourcePath, $"{item.Name}: {ex.Message}"));
            }
        }

        PurgeEmptyStaging(outcome);
        return new DeleteUndoResult(restored, failed);
    }

    // --- Staging lifecycle ---

    /// <summary>
    /// Erases everything an outcome is still holding, committing the delete. Call this only once
    /// the outcome can no longer be undone: up to that point the held copies are the only ones
    /// there are. Guarded so it can only ever remove a folder this class made.
    /// </summary>
    public static void CommitStaging(DeleteOutcome outcome)
    {
        foreach (var directory in outcome.StagingDirectories)
        {
            if (!IsStagingDirectory(directory)) continue;
            try
            {
                if (Directory.Exists(directory)) Directory.Delete(directory, recursive: true);
                RemoveEmptyTrashRoot(directory);
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                // A holding folder that survives is only wasted space; never let cleanup throw.
            }
        }
    }

    /// <summary>Removes an outcome's holding folders only if they are empty — used after an undo,
    /// where anything left behind is an item that could not be put back and must not be erased
    /// along with the folder.</summary>
    private static void PurgeEmptyStaging(DeleteOutcome outcome)
    {
        foreach (var directory in outcome.StagingDirectories)
        {
            if (!IsStagingDirectory(directory)) continue;
            try
            {
                if (!Directory.Exists(directory)) continue;
                if (Directory.EnumerateFileSystemEntries(directory).Any()) continue;
                Directory.Delete(directory);
                RemoveEmptyTrashRoot(directory);
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
            }
        }
    }

    /// <summary>Everything an outcome is still holding — surfaced so a failed undo can say where
    /// the data went instead of leaving it to be found by accident.</summary>
    public static IReadOnlyList<string> StagedItems(DeleteOutcome outcome)
    {
        var items = new List<string>();
        foreach (var directory in outcome.StagingDirectories)
        {
            try
            {
                if (Directory.Exists(directory)) items.AddRange(Directory.EnumerateFileSystemEntries(directory));
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
            }
        }
        return items;
    }

    /// <summary>
    /// Erases holding folders left behind by a previous session — a crash, or a kill — from every
    /// ready volume. Only batches older than <paramref name="olderThan"/> go, so a second copy of
    /// the app running right now cannot have its pending undo swept out from under it.
    /// </summary>
    /// <remarks>Best-effort and silent: a drive that refuses to be read is simply skipped.</remarks>
    public static void PurgeAbandonedStaging(TimeSpan olderThan) =>
        PurgeAbandonedStaging(olderThan, ReadyVolumeRoots());

    /// <inheritdoc cref="PurgeAbandonedStaging(TimeSpan)"/>
    /// <param name="roots">Where to look for holding folders.</param>
    public static void PurgeAbandonedStaging(TimeSpan olderThan, IEnumerable<string> roots)
    {
        var cutoff = DateTime.UtcNow - olderThan;

        foreach (var root in roots)
        {
            string trash;
            try
            {
                trash = Path.Combine(root, TrashFolderName);
                if (!Directory.Exists(trash)) continue;
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                continue;
            }

            try
            {
                foreach (var batch in Directory.EnumerateDirectories(trash, BatchPrefix + "*"))
                {
                    try
                    {
                        if (Directory.GetCreationTimeUtc(batch) > cutoff) continue;
                        Directory.Delete(batch, recursive: true);
                    }
                    catch (Exception ex) when (IsDeleteFailure(ex))
                    {
                    }
                }
                if (!Directory.EnumerateFileSystemEntries(trash).Any()) Directory.Delete(trash);
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
            }
        }
    }

    /// <summary>Every volume that can be read right now. A drive that is not ready — an empty card
    /// reader, a disconnected share — is skipped rather than waited on.</summary>
    private static IEnumerable<string> ReadyVolumeRoots()
    {
        DriveInfo[] drives;
        try
        {
            drives = DriveInfo.GetDrives();
        }
        catch (Exception ex) when (IsDeleteFailure(ex))
        {
            yield break;
        }

        foreach (var drive in drives)
        {
            string root;
            try
            {
                if (!drive.IsReady) continue;
                root = drive.RootDirectory.FullName;
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                continue;
            }
            yield return root;
        }
    }

    /// <summary>
    /// True for anything sitting in a holding folder — i.e. deleted, but not yet committed. Search
    /// asks, because an item the user has just deleted turning up in results looks exactly like a
    /// delete that did not work.
    /// </summary>
    public static bool IsHeldPath(string path)
    {
        foreach (var segment in path.Split(['\\', '/'], StringSplitOptions.RemoveEmptyEntries))
        {
            if (string.Equals(segment, TrashFolderName, StringComparison.OrdinalIgnoreCase) ||
                segment.StartsWith(LocalStagingPrefix, StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }

    /// <summary>The guard that makes the recursive deletes above safe: a path only counts if it is
    /// named the way this class names its holding folders.</summary>
    private static bool IsStagingDirectory(string path)
    {
        var name = Path.GetFileName(Path.TrimEndingDirectorySeparator(path));
        if (name.StartsWith(LocalStagingPrefix, StringComparison.Ordinal)) return true;

        return name.StartsWith(BatchPrefix, StringComparison.Ordinal) &&
            string.Equals(
                Path.GetFileName(Path.GetDirectoryName(path)), TrashFolderName, StringComparison.Ordinal);
    }

    /// <summary>Takes the per-volume trash folder away once its last batch has gone, so a disk that
    /// is not mid-delete carries no trace of one.</summary>
    private static void RemoveEmptyTrashRoot(string batchDirectory)
    {
        var trash = Path.GetDirectoryName(batchDirectory);
        if (trash is null ||
            !string.Equals(Path.GetFileName(trash), TrashFolderName, StringComparison.Ordinal)) return;
        try
        {
            if (Directory.Exists(trash) && !Directory.EnumerateFileSystemEntries(trash).Any())
                Directory.Delete(trash);
        }
        catch (Exception ex) when (IsDeleteFailure(ex))
        {
        }
    }

    // --- Filesystem primitives ---

    private static string CreateStagingDirectory(string path)
    {
        Directory.CreateDirectory(path);
        try
        {
            File.SetAttributes(path, File.GetAttributes(path) | FileAttributes.Hidden);
        }
        catch (Exception ex) when (IsDeleteFailure(ex))
        {
            // Visible staging is ugly but not wrong.
        }
        return path;
    }

    /// <summary>"name (2)"-style free path. Directories number the whole name; files number before
    /// the extension.</summary>
    private string UniquePath(string path)
    {
        if (!Exists(path)) return path;

        var directory = Path.GetDirectoryName(path) ?? "";
        var name = Path.GetFileName(path);
        var isDirectory = _probe.DirectoryExists(path);
        var stem = isDirectory ? name : Path.GetFileNameWithoutExtension(name);
        var extension = isDirectory ? "" : Path.GetExtension(name);

        for (var i = 2; ; i++)
        {
            var candidate = Path.Combine(directory, $"{stem} ({i}){extension}");
            if (!Exists(candidate)) return candidate;
        }
    }

    private static string ShortId() => Guid.NewGuid().ToString("N")[..8];

    private bool Exists(string path) => _probe.FileExists(path) || _probe.DirectoryExists(path);

    /// <summary>Errors that mean "this item failed" rather than "the program is broken".</summary>
    private static bool IsDeleteFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException;
}
