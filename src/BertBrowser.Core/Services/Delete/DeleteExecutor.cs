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
    private readonly IRecycleBin? _recycleBin;
    private readonly IRecycleProbe _recycleProbe;

    /// <param name="probe">How the executor asks what is on disk.</param>
    /// <param name="protectedPaths">Locations to refuse outright; defaults to
    /// <see cref="ProtectedLocations.Default"/>.</param>
    /// <param name="stagingRoot">Where to put the holding folder, instead of the root of each
    /// item's own volume. Only for tests, which have no business creating folders at the root of
    /// the machine's disks — and, being under the temp directory, still land on the same volume as
    /// the files they make, which is what the whole design rests on.</param>
    /// <param name="recycleBin">The Windows Recycle Bin. Null means there is none to use, and every
    /// item the plan wanted recycled is held in the staging folder instead — never erased.</param>
    /// <param name="recycleProbe">Re-asks, against live state, whether an item's volume will take a
    /// recycled item; defaults to "no volume will".</param>
    public DeleteExecutor(
        IDeleteProbe probe,
        IEnumerable<string>? protectedPaths = null,
        string? stagingRoot = null,
        IRecycleBin? recycleBin = null,
        IRecycleProbe? recycleProbe = null)
    {
        _probe = probe;
        _protectedKeys = protectedPaths is null
            ? ProtectedLocations.Default
            : ProtectedLocations.KeysOf(protectedPaths);
        _stagingRoot = stagingRoot;
        _recycleBin = recycleBin;
        _recycleProbe = recycleProbe ?? NoRecycleProbe.Instance;
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

        // Items bound for the Recycle Bin are gathered rather than done here: the shell wants one
        // operation with everything added to it, which is also the only way to get a single
        // progress sink out of it.
        var toRecycle = new List<PlannedDelete>();

        var total = plan.Deletions.Count;
        var done = 0;
        foreach (var item in plan.Deletions)
        {
            if (ct.IsCancellationRequested) break;

            try
            {
                Revalidate(item);

                switch (LiveDisposition(item))
                {
                    case DeleteDisposition.Recycle:
                        toRecycle.Add(item);
                        continue; // counted when the batch runs

                    case DeleteDisposition.Erase:
                        progress?.Report(new DeleteProgress(done, total, item.Name));
                        done++;
                        Erase(item.SourcePath, item.IsDirectory);
                        deleted.Add(new DeletedItem(item.SourcePath, item.IsDirectory, null));
                        break;

                    default:
                        progress?.Report(new DeleteProgress(done, total, item.Name));
                        done++;
                        var staged = Stage(item, volumeBatches, localBatches, staging);
                        deleted.Add(new DeletedItem(item.SourcePath, item.IsDirectory, staged));
                        break;
                }
            }
            catch (Exception ex) when (IsDeleteFailure(ex))
            {
                failed.Add(new FailedDelete(item.SourcePath, $"{item.Name}: {ex.Message}"));
            }
        }

        if (toRecycle.Count > 0 && _recycleBin is { } bin && !ct.IsCancellationRequested)
        {
            var result = bin.Recycle(toRecycle, ct, Offset(progress, done, total));
            foreach (var item in result.Recycled)
                deleted.Add(new DeletedItem(item.SourcePath, item.IsDirectory, null, item.RecycledPath));
            failed.AddRange(result.Failed);
            done += toRecycle.Count;
        }

        progress?.Report(new DeleteProgress(done, total, ""));
        return new DeleteOutcome(plan.Permanent, deleted, failed, staging);
    }

    /// <summary>
    /// The planner's routing, re-asked against live state — a plan is built while the confirmation
    /// is open. An item can lose its Recycle Bin between the two (a share goes away, the bin is
    /// turned off), and with no bin to hand it to the answer is the holding folder, never an erase.
    /// </summary>
    private DeleteDisposition LiveDisposition(PlannedDelete item)
    {
        if (item.Disposition != DeleteDisposition.Recycle) return item.Disposition;

        return _recycleBin is not null && _recycleProbe.CanRecycle(item.SourcePath)
            ? DeleteDisposition.Recycle
            : DeleteDisposition.Stage;
    }

    /// <summary>Shifts a batch's own 0..n progress into its place in the whole plan, so the bar does
    /// not restart when the Recycle Bin takes over.</summary>
    private static IProgress<DeleteProgress>? Offset(
        IProgress<DeleteProgress>? progress, int done, int total) =>
        progress is null ? null : new OffsetProgress(progress, done, total);

    private sealed class OffsetProgress(IProgress<DeleteProgress> inner, int done, int total)
        : IProgress<DeleteProgress>
    {
        public void Report(DeleteProgress value) =>
            inner.Report(new DeleteProgress(done + value.Done, total, value.CurrentName));
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

    /// <summary>
    /// <see cref="DirectoryRemoval.RemoveTree"/>, not <c>Directory.Delete(recursive: true)</c>: that
    /// call erases the rest of a tree containing a junction and then throws naming the link, which on
    /// this path means a permanent delete destroying the contents and reporting that it failed.
    /// </summary>
    private static void Remove(string path, bool isDirectory)
    {
        if (isDirectory) DirectoryRemoval.RemoveTree(path);
        else File.Delete(path);
    }

    /// <summary>
    /// Best-effort: anything still read-only afterwards fails the retry, which is reported.
    /// </summary>
    /// <remarks>
    /// <b>The walk descends by hand rather than through <c>SearchOption.AllDirectories</c>, and that
    /// is the whole point of it.</b> That overload follows directory reparse points, so a junction
    /// anywhere in the tree put this loop into a directory somewhere else entirely — and a test for
    /// the reparse bit on each entry does not help, because by the time the entry arrives the
    /// enumeration has already gone through the link. Clearing read-only outside the tree being
    /// deleted is a small harm (<see cref="Remove"/> does not follow junctions, so nothing out there
    /// is erased) but it is a harm to files the user never selected. An explicit stack that declines
    /// to push a link is the same shape <see cref="DeleteSurveyor"/> walks with, and for the same
    /// reason: a junction is the one entry deleting it removes.
    /// </remarks>
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
            if (DirectoryRemoval.IsLink(root)) return;

            var pending = new Stack<DirectoryInfo>();
            pending.Push(root);
            while (pending.Count > 0)
            {
                var current = pending.Pop();
                foreach (var entry in current.GetFileSystemInfos())
                {
                    try
                    {
                        if (entry is DirectoryInfo child && !DirectoryRemoval.IsLink(child)) pending.Push(child);
                        if (DirectoryRemoval.IsLink(entry)) continue;
                        if ((entry.Attributes & FileAttributes.ReadOnly) != 0)
                            entry.Attributes &= ~FileAttributes.ReadOnly;
                    }
                    catch (Exception ex) when (IsDeleteFailure(ex))
                    {
                        // One stubborn entry must not stop the rest; the delete retry reports it.
                    }
                }
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
                // Nothing here writes over anything, whichever way the item was held.
                if (Exists(item.SourcePath))
                {
                    failed.Add(new FailedDelete(item.SourcePath,
                        $"{item.Name}: something else now occupies its original location."));
                    continue;
                }

                if (item.RecycledPath is not null)
                {
                    if (_recycleBin is not { } bin || !bin.Restore(item))
                    {
                        failed.Add(new FailedDelete(item.SourcePath,
                            $"{item.Name}: the Recycle Bin no longer holds it ({item.RecycledPath})."));
                        continue;
                    }
                    restored++;
                    continue;
                }

                if (item.StagedPath is not { } staged || !Exists(staged))
                {
                    failed.Add(new FailedDelete(item.SourcePath,
                        $"{item.Name}: the deleted copy is no longer being held."));
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
                // RemoveTree, not Directory.Delete(recursive): a held item can be a tree with a
                // junction in it, and that call would erase most of the folder and then throw —
                // caught here, so the debris would simply be left behind without a word.
                if (Directory.Exists(directory)) DirectoryRemoval.RemoveTree(directory);
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
                        DirectoryRemoval.RemoveTree(batch);
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
    /// True for anything sitting in a holding folder or in the Windows Recycle Bin — i.e. deleted,
    /// whether this app is holding it or the shell is. Search asks, because an item the user has
    /// just deleted turning up in results looks exactly like a delete that did not work; and a
    /// recycled file surfacing as <c>C:\$Recycle.Bin\S-1-5-21-…\$RAB1234.txt</c> is worse still,
    /// since the name it turns up under is not even the one that was deleted.
    /// </summary>
    public static bool IsHeldPath(string path)
    {
        if (ProtectedLocations.IsInsideRecycleBin(path)) return true;

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

    /// <summary>"name (2)"-style free path, through this executor's probe.</summary>
    private string UniquePath(string path) =>
        Paths.UniquePath.For(
            path, _probe.DirectoryExists(path), _probe.DirectoryExists, _probe.FileExists);

    private static string ShortId() => Guid.NewGuid().ToString("N")[..8];

    private bool Exists(string path) => _probe.FileExists(path) || _probe.DirectoryExists(path);

    /// <summary>Errors that mean "this item failed" rather than "the program is broken".</summary>
    private static bool IsDeleteFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException;
}
