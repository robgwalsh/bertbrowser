using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>Restoring a transfer: how many items went back, and what could not.</summary>
public sealed record TransferUndoResult(int Restored, IReadOnlyList<FailedTransfer> Failed);

/// <summary>
/// Carries out a <see cref="TransferPlan"/>. Every rule the planner applied is re-checked here
/// against live disk state, because a plan is built when the drag hovers and executed when it
/// drops — the filesystem can change in between.
/// </summary>
/// <remarks>
/// The invariants this class exists to hold:
/// <list type="bullet">
/// <item>Nothing is ever deleted to make room. <see cref="ConflictResolution.Replace"/> moves the
/// displaced entry into a hidden staging folder, so an undo can put it back.</item>
/// <item>A cross-volume directory move copies, verifies the copy matches the source by file count
/// and total bytes, and only then deletes the source. A verification failure removes the partial
/// copy and leaves the source untouched.</item>
/// <item>A directory tree containing junctions or symlinks is refused across volumes rather than
/// copied without them and then deleted.</item>
/// <item>A failure on one item never aborts or rolls back the others; each is independent.</item>
/// <item>A cancel takes effect <em>inside</em> a file, not merely between items, and leaves nothing
/// half-written where a finished file belongs. Whatever got across before it stays across, and a
/// cancelled move is still undoable.</item>
/// </list>
/// </remarks>
public sealed class TransferExecutor
{
    /// <summary>ERROR_NOT_SAME_DEVICE as an HRESULT — the only <see cref="IOException"/> from
    /// <see cref="Directory.Move"/> that may be answered with copy-then-delete. Every other one
    /// (sharing violation, access denied, target exists) must surface as a failure.</summary>
    private const int HResultNotSameDevice = unchecked((int)0x80070011);

    internal const string StagingPrefix = ".bertbrowser-replaced-";

    private readonly ITransferProbe _probe;
    private readonly IFileCopier _copier;

    public TransferExecutor(ITransferProbe probe, IFileCopier copier)
    {
        _probe = probe;
        _copier = copier;
    }

    public TransferExecutor(ITransferProbe probe) : this(probe, new FileSystemFileCopier())
    {
    }

    public TransferExecutor() : this(new FileSystemTransferProbe(), new FileSystemFileCopier())
    {
    }

    /// <param name="plan">The plan to carry out.</param>
    /// <param name="resolutions">How to settle conflicts, keyed by <see cref="PathKey.Canonicalize"/>
    /// of the source path. Anything unlisted falls back to the non-destructive
    /// <see cref="ConflictResolution.KeepBoth"/>.</param>
    public TransferOutcome Execute(
        TransferPlan plan,
        IReadOnlyDictionary<string, ConflictResolution>? resolutions = null,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null)
    {
        var completed = new List<CompletedTransfer>();
        var skipped = new List<string>();
        var failed = new List<FailedTransfer>();
        string? stagingDirectory = null;
        var cancelled = false;

        var run = new ProgressCoalescer(ct, progress, plan.Transfers.Count);
        foreach (var transfer in plan.Transfers)
        {
            if (ct.IsCancellationRequested)
            {
                cancelled = true;
                break;
            }

            run.BeginItem(transfer.Name);
            try
            {
                var resolution = Resolution(resolutions, transfer, plan.Verb);
                var result = ExecuteOne(plan, transfer, resolution, ref stagingDirectory, run);
                if (result is null) skipped.Add(transfer.SourcePath);
                else completed.Add(result);
            }
            catch (OperationCanceledException)
            {
                // Stopped part-way through this item. Everything it had started is already undone
                // by the primitive that was interrupted, so there is nothing to report about it.
                cancelled = true;
                break;
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
                failed.Add(new FailedTransfer(
                    transfer.SourcePath, $"{transfer.Name}: {ex.Message}", AccessDenied.Caused(ex)));
            }
            run.EndItem();
        }

        run.Finished();
        return new TransferOutcome(
            plan.Verb,
            plan.DestinationDirectory,
            completed,
            skipped,
            failed,
            stagingDirectory is null ? [] : [stagingDirectory],
            cancelled);
    }

    /// <summary>Returns the completed record, or null when the item was skipped.</summary>
    private CompletedTransfer? ExecuteOne(
        TransferPlan plan, PlannedTransfer transfer, ConflictResolution resolution,
        ref string? stagingDirectory, ProgressCoalescer run)
    {
        Revalidate(plan, transfer);

        var destinationPath = transfer.DestinationPath;
        string? displacedStagePath = null;

        // The plan's conflict flag is a snapshot; ask disk again at the moment of the write.
        if (Exists(destinationPath))
        {
            switch (resolution)
            {
                case ConflictResolution.Skip:
                    return null;

                // One arm for both, because they write the same thing: the difference between them
                // is whether the caller can undo the result, which is settled before we get here.
                case ConflictResolution.Replace:
                case ConflictResolution.Overwrite:
                    stagingDirectory ??= CreateStagingDirectory(plan.DestinationDirectory);
                    displacedStagePath = Path.Combine(stagingDirectory, Path.GetFileName(destinationPath));
                    displacedStagePath = UniquePath(displacedStagePath);
                    // A rename within the same directory tree: instant, and reversible by undo.
                    // Deliberately on an uncancellable run — clearing the name is bookkeeping, and
                    // a cancel landing half-way through it would strand the displaced entry.
                    MoveEntry(destinationPath, displacedStagePath, Directory.Exists(destinationPath), ProgressCoalescer.Silent());
                    break;

                default:
                    destinationPath = UniquePath(destinationPath);
                    break;
            }
        }

        try
        {
            if (plan.Verb == TransferVerb.Move)
                MoveEntry(transfer.SourcePath, destinationPath, transfer.IsDirectory, run);
            else
                CopyEntry(transfer.SourcePath, destinationPath, transfer.IsDirectory, run);
        }
        catch when (displacedStagePath is not null)
        {
            // The name was cleared for a transfer that then failed or was cancelled. Nothing
            // succeeded, so the displaced entry goes straight back — no undo record will exist to
            // rescue it later.
            RestoreDisplaced(displacedStagePath, destinationPath);
            throw;
        }

        return new CompletedTransfer(
            transfer.SourcePath, destinationPath, transfer.IsDirectory, displacedStagePath);
    }

    /// <summary>
    /// Puts a staged entry back under the name it was displaced from. Never throws: it runs while
    /// another failure is being propagated, and must not mask it. If even this fails the entry is
    /// still in staging, which is reported by <see cref="StagedItems"/>.
    /// </summary>
    private void RestoreDisplaced(string stagePath, string destinationPath)
    {
        try
        {
            if (Exists(stagePath) && !Exists(destinationPath))
                MoveEntry(stagePath, destinationPath, Directory.Exists(stagePath), ProgressCoalescer.Silent());
        }
        catch (Exception ex) when (IsTransferFailure(ex))
        {
        }
    }

    /// <summary>Re-applies the planner's data-safety rules against live disk state.</summary>
    private void Revalidate(TransferPlan plan, PlannedTransfer transfer)
    {
        if (!Exists(transfer.SourcePath))
            throw new FileNotFoundException($"'{transfer.Name}' no longer exists.", transfer.SourcePath);
        if (!_probe.DirectoryExists(plan.DestinationDirectory))
            throw new DirectoryNotFoundException("The destination folder no longer exists.");

        if (!transfer.IsDirectory) return;

        var sourceKey = PathKey.Canonicalize(transfer.SourcePath);
        var destKey = PathKey.Canonicalize(plan.DestinationDirectory);
        var sourceResolved = PathKey.Canonicalize(_probe.ResolveFinalPath(transfer.SourcePath));
        var destResolved = PathKey.Canonicalize(_probe.ResolveFinalPath(plan.DestinationDirectory));

        if (destKey == sourceKey || destResolved == sourceResolved ||
            PathKey.IsUnder(destKey, sourceKey) || PathKey.IsUnder(destResolved, sourceResolved) ||
            PathKey.IsUnder(destResolved, sourceKey) || PathKey.IsUnder(destKey, sourceResolved))
            throw new IOException($"Cannot move '{transfer.Name}' into itself or one of its subfolders.");
    }

    private static ConflictResolution Resolution(
        IReadOnlyDictionary<string, ConflictResolution>? resolutions, PlannedTransfer transfer, TransferVerb verb)
    {
        var chosen = ConflictResolution.KeepBoth;
        if (resolutions is not null &&
            resolutions.TryGetValue(PathKey.Canonicalize(transfer.SourcePath), out var value))
            chosen = value;

        // Copy is defined as purely additive, so it must never displace an existing entry.
        return chosen == ConflictResolution.Replace && verb == TransferVerb.Copy
            ? ConflictResolution.KeepBoth
            : chosen;
    }

    // --- Undo ---

    /// <summary>
    /// Puts a completed move back: each item returns to the path it came from, then anything it
    /// displaced is restored from staging. Items whose original path has since been taken are
    /// reported rather than overwritten.
    /// </summary>
    public TransferUndoResult Undo(TransferOutcome outcome, CancellationToken ct = default)
    {
        if (outcome.Verb != TransferVerb.Move)
            return new TransferUndoResult(0, [new FailedTransfer("", "Only a move can be undone.")]);

        var failed = new List<FailedTransfer>();
        var restored = 0;

        // Reverse order so nested transfers unwind in the order they were made.
        foreach (var item in outcome.Completed.Reverse())
        {
            if (ct.IsCancellationRequested) break;
            var name = Path.GetFileName(item.SourcePath);
            try
            {
                if (!Exists(item.FinalPath))
                {
                    failed.Add(new FailedTransfer(item.SourcePath, $"{name}: it is no longer where it was moved to."));
                    continue;
                }
                if (Exists(item.SourcePath))
                {
                    failed.Add(new FailedTransfer(item.SourcePath,
                        $"{name}: something else now occupies its original location."));
                    continue;
                }

                var parent = Path.GetDirectoryName(item.SourcePath);
                if (parent is null || !_probe.DirectoryExists(parent))
                {
                    failed.Add(new FailedTransfer(item.SourcePath, $"{name}: its original folder is gone."));
                    continue;
                }

                MoveEntry(item.FinalPath, item.SourcePath, item.IsDirectory, ProgressCoalescer.Silent());
                restored++;

                // The name it took over is free again: put the displaced entry back.
                if (item.DisplacedStagePath is { } staged && Exists(staged) && !Exists(item.FinalPath))
                    MoveEntry(staged, item.FinalPath, Directory.Exists(staged), ProgressCoalescer.Silent());
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
                failed.Add(new FailedTransfer(item.SourcePath, $"{name}: {ex.Message}", AccessDenied.Caused(ex)));
            }
        }

        PurgeStaging(outcome);
        return new TransferUndoResult(restored, failed);
    }

    /// <summary>
    /// Undoes a completed <em>copy</em>: what it added is removed, and what it displaced is put
    /// back. <see cref="Undo"/> refuses a copy; this is the door a caller that kept the outcome
    /// and means to reverse the whole thing comes in by.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Removing what a copy added is not a breach of "nothing is ever deleted to make room".</b>
    /// That rule is about displacing something already there, and it still holds: the displaced
    /// entry is in staging and comes back here. What goes is only what this run wrote — the same
    /// thing a cancelled copy erases, for the same reason. Every path removed is one
    /// <see cref="ExecuteOne"/> created: an <see cref="ConflictResolution.Overwrite"/> cleared the
    /// name into staging before writing, and every other resolution wrote to a name that was free,
    /// so no destination here ever held anything but this run's own bytes.
    /// </para>
    /// <para>
    /// It is still a real undo of a real write, so an edit made to a copied file between the copy
    /// and the undo goes with it — as it would when undoing a paste anywhere else. The window is
    /// one operation wide, because there is one undo slot.
    /// </para>
    /// <para>
    /// Removal goes through <see cref="DirectoryRemoval.RemoveTree"/> rather than
    /// <c>Directory.Delete(recursive: true)</c>, which erases the rest of a tree before throwing
    /// when it meets a junction — and a copied tree can hold one.
    /// </para>
    /// </remarks>
    public TransferUndoResult UndoCopies(TransferOutcome outcome, CancellationToken ct = default)
    {
        if (outcome.Verb != TransferVerb.Copy)
            return new TransferUndoResult(0, [new FailedTransfer("", "Only a copy can be undone here.")]);

        var failed = new List<FailedTransfer>();
        var restored = 0;

        // Reverse order, so a folder is not removed out from under the entries copied into it.
        foreach (var item in outcome.Completed.Reverse())
        {
            if (ct.IsCancellationRequested) break;
            var name = Path.GetFileName(item.FinalPath);
            try
            {
                if (Exists(item.FinalPath))
                {
                    if (item.IsDirectory) DirectoryRemoval.RemoveTree(item.FinalPath);
                    else File.Delete(item.FinalPath);
                }

                // The name is free again: put back whatever the copy took it from.
                if (item.DisplacedStagePath is { } staged && Exists(staged) && !Exists(item.FinalPath))
                    MoveEntry(staged, item.FinalPath, Directory.Exists(staged), ProgressCoalescer.Silent());

                restored++;
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
                failed.Add(new FailedTransfer(item.FinalPath, $"{name}: {ex.Message}", AccessDenied.Caused(ex)));
            }
        }

        PurgeStaging(outcome);
        return new TransferUndoResult(restored, failed);
    }

    /// <summary>
    /// Deletes an outcome's staging folder once it can no longer be undone. Refuses any path that
    /// is not a staging folder this class created, and leaves a non-empty one alone — losing the
    /// ability to undo must never turn into losing the data.
    /// </summary>
    public void PurgeStaging(TransferOutcome outcome)
    {
        foreach (var staging in outcome.StagingDirectories)
        {
            if (!IsStagingDirectory(staging)) continue;

            try
            {
                if (!Directory.Exists(staging)) continue;
                if (Directory.EnumerateFileSystemEntries(staging).Any()) continue; // still holds displaced data
                Directory.Delete(staging);
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
                // A leftover hidden folder is harmless; never let cleanup break the operation.
            }
        }
    }

    /// <summary>
    /// Discards an outcome's staging folder <em>and anything still in it</em>, committing the
    /// replacements it represents. Call this only when the outcome can no longer be undone — up to
    /// that point the displaced entries are the only copy left. Guarded so it can never delete
    /// anything other than a staging folder <see cref="CreateStagingDirectory"/> made.
    /// </summary>
    public static void CommitStaging(TransferOutcome outcome)
    {
        foreach (var staging in outcome.StagingDirectories)
        {
            if (!IsStagingDirectory(staging)) continue;

            try
            {
                // DirectoryRemoval.RemoveTree, not Directory.Delete(recursive: true). What a Replace
                // displaced into staging is the user's own folder and can contain a junction, and that
                // call erases the rest of such a tree before throwing — swallowed here as harmless
                // cleanup, so half a folder would be left behind for good without a word.
                if (Directory.Exists(staging)) DirectoryRemoval.RemoveTree(staging);
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
                // A leftover hidden folder is harmless; never let cleanup break the operation.
            }
        }
    }

    /// <summary>Everything still parked in staging for an outcome that is no longer undoable.
    /// Surfaced so the user can be told where displaced items went instead of losing them.</summary>
    public static IReadOnlyList<string> StagedItems(TransferOutcome outcome)
    {
        var items = new List<string>();
        foreach (var staging in outcome.StagingDirectories)
        {
            if (!IsStagingDirectory(staging) || !Directory.Exists(staging)) continue;
            try
            {
                items.AddRange(Directory.EnumerateFileSystemEntries(staging));
            }
            catch (Exception ex) when (IsTransferFailure(ex))
            {
            }
        }
        return items;
    }

    /// <summary>
    /// Whether a path is a staging folder <see cref="CreateStagingDirectory"/> made — the guard on
    /// every recursive delete in this file. A path only qualifies if it is named the way this class
    /// names them, so a mangled or hand-built outcome cannot direct the erase at a real folder.
    /// </summary>
    private static bool IsStagingDirectory(string path) =>
        Path.GetFileName(path).StartsWith(StagingPrefix, StringComparison.Ordinal);

    // --- Filesystem primitives ---

    private void MoveEntry(string source, string destination, bool isDirectory, ProgressCoalescer run)
    {
        if (!isDirectory)
        {
            // Spans volumes by itself, so unlike Directory.Move there is no ERROR_NOT_SAME_DEVICE
            // arm to write here. Within a volume it is a rename and reports no bytes at all, which
            // is why a same-volume move shows no progress rather than a bar that never moves.
            run.BeginFile(0);
            _copier.Move(source, destination, run.FileProgress, run.Token);
            run.EndFile();
            return;
        }

        if (SameVolume(source, destination))
        {
            Directory.Move(source, destination);
            return;
        }

        try
        {
            Directory.Move(source, destination);
        }
        catch (IOException ex) when (ex.HResult == HResultNotSameDevice)
        {
            // Mount points can make two paths share a root string but not a volume.
            CrossVolumeMoveDirectory(source, destination, run);
        }
    }

    private void CopyEntry(string source, string destination, bool isDirectory, ProgressCoalescer run)
    {
        if (!isDirectory)
        {
            CopyFile(source, destination, TryLength(source), run);
            return;
        }

        try
        {
            CopyDirectory(new DirectoryInfo(source), destination, run);
        }
        catch (OperationCanceledException)
        {
            // A copy is defined as purely additive, so a cancelled one must add nothing: half a
            // tree at the destination reads as a finished copy to everything that looks at it.
            TryDeletePartialCopy(destination);
            throw;
        }
    }

    /// <summary>One file's bytes, with the run's counters bracketing them.</summary>
    /// <param name="knownLength">The size we already know, or null to take the OS's word for it.
    /// Passing it keeps the running total exact even when the copy reports coarsely.</param>
    private void CopyFile(string source, string destination, long? knownLength, ProgressCoalescer run)
    {
        run.BeginFile(knownLength ?? 0);
        _copier.Copy(source, destination, run.FileProgress, run.Token);
        run.EndFile();
    }

    private static long? TryLength(string path)
    {
        try
        {
            var info = new FileInfo(path);
            return info.Exists ? info.Length : null;
        }
        catch (Exception ex) when (IsTransferFailure(ex))
        {
            return null;
        }
    }

    /// <summary>
    /// Copy, verify, then delete. The source is only removed once the destination is confirmed to
    /// hold the same number of files and the same total bytes.
    /// </summary>
    private void CrossVolumeMoveDirectory(string source, string destination, ProgressCoalescer run)
    {
        var info = new DirectoryInfo(source);

        // Junctions and symlinks cannot be reproduced by copying, and deleting the source after a
        // copy that dropped them would destroy them. Refuse instead.
        if (FindReparsePoint(info) is { } link)
            throw new IOException(
                $"'{info.Name}' contains a junction or symbolic link ({Path.GetFileName(link)}) and cannot be " +
                "moved to another drive. Move it within the same drive, or copy it and remove the original.");

        var expected = Measure(info);

        try
        {
            CopyDirectory(info, destination, run);
        }
        catch
        {
            // Covers a cancel as well as a failure: either way the source is still there and the
            // half-copy at the destination is not something anyone should be left holding.
            TryDeletePartialCopy(destination);
            throw;
        }

        var actual = Measure(new DirectoryInfo(destination));
        if (actual != expected)
        {
            TryDeletePartialCopy(destination);
            throw new IOException(
                $"Verification failed while moving '{info.Name}' to another drive " +
                $"(expected {expected.Files:N0} files / {expected.Bytes:N0} bytes, " +
                $"found {actual.Files:N0} / {actual.Bytes:N0}). Nothing was removed.");
        }

        Directory.Delete(source, recursive: true);
    }

    private static (int Files, long Bytes) Measure(DirectoryInfo root)
    {
        var files = 0;
        var bytes = 0L;
        foreach (var file in root.EnumerateFiles("*", SearchOption.AllDirectories))
        {
            files++;
            bytes += file.Length;
        }
        return (files, bytes);
    }

    /// <summary>The first junction/symlink anywhere in the tree, or null.</summary>
    private static string? FindReparsePoint(DirectoryInfo root)
    {
        foreach (var entry in root.EnumerateFileSystemInfos())
        {
            if ((entry.Attributes & FileAttributes.ReparsePoint) != 0) return entry.FullName;
            if (entry is DirectoryInfo child && FindReparsePoint(child) is { } found) return found;
        }
        return null;
    }

    private static void TryDeletePartialCopy(string destination)
    {
        try
        {
            // DirectoryRemoval.RemoveTree rather than Directory.Delete(recursive: true), for the
            // reason that call is banned everywhere else in this app: given a junction it erases
            // the rest of the tree and then throws — swallowed here as harmless cleanup, so half a
            // partial copy would be left behind without a word.
            if (Directory.Exists(destination)) DirectoryRemoval.RemoveTree(destination);
        }
        catch (Exception ex) when (IsTransferFailure(ex))
        {
            // Best effort: the source is still intact, which is what matters.
        }
    }

    private void CopyDirectory(DirectoryInfo source, string destination, ProgressCoalescer run)
    {
        run.Token.ThrowIfCancellationRequested();
        Directory.CreateDirectory(destination);
        foreach (var entry in source.EnumerateFileSystemInfos())
        {
            run.Token.ThrowIfCancellationRequested();

            if ((entry.Attributes & FileAttributes.ReparsePoint) != 0)
                throw new IOException(
                    $"'{entry.Name}' is a junction or symbolic link and cannot be copied.");

            if (entry is DirectoryInfo dir)
                CopyDirectory(dir, Path.Combine(destination, dir.Name), run);
            else if (entry is FileInfo file)
                // The length comes free from the enumeration that found the file, so the running
                // total is exact without a stat per entry.
                CopyFile(file.FullName, Path.Combine(destination, file.Name), file.Length, run);
        }
        try
        {
            Directory.SetLastWriteTimeUtc(destination, source.LastWriteTimeUtc);
        }
        catch (Exception ex) when (IsTransferFailure(ex))
        {
            // Cosmetic only.
        }
    }

    /// <summary>Conservative: false whenever the volumes cannot be established, which routes the
    /// move through the guarded fallback instead of assuming a plain rename will do.</summary>
    /// <remarks>Internal because <see cref="TransferEstimator"/> has to answer the same question to
    /// know whether an item moves any bytes at all, and the two must not drift apart.</remarks>
    internal static bool SameVolume(string a, string b)
    {
        try
        {
            var rootA = Path.GetPathRoot(Path.GetFullPath(a));
            var rootB = Path.GetPathRoot(Path.GetFullPath(b));
            return rootA is { Length: > 0 } && rootB is { Length: > 0 }
                && rootA.Equals(rootB, StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return false;
        }
    }

    private string CreateStagingDirectory(string destinationDirectory)
    {
        var path = Path.Combine(destinationDirectory, StagingPrefix + Guid.NewGuid().ToString("N")[..8]);
        Directory.CreateDirectory(path);
        try
        {
            File.SetAttributes(path, File.GetAttributes(path) | FileAttributes.Hidden);
        }
        catch (Exception ex) when (IsTransferFailure(ex))
        {
            // Visible staging is ugly but not wrong.
        }
        return path;
    }

    /// <summary>"name (2)"-style free path, through this executor's probe.</summary>
    private string UniquePath(string path) =>
        Paths.UniquePath.For(
            path, _probe.DirectoryExists(path), _probe.DirectoryExists, _probe.FileExists);

    private bool Exists(string path) => _probe.FileExists(path) || _probe.DirectoryExists(path);

    /// <summary>Errors that mean "this item failed" rather than "the program is broken".</summary>
    private static bool IsTransferFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException;
}
