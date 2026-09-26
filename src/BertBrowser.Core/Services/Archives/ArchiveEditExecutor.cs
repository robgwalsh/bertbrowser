using BertBrowser.Core.Services.Transfer;
using SharpCompress.Common;
using SharpCompress.Writers;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Carries out an <see cref="ArchiveEditPlan"/> by rewriting the container beside itself and
/// swapping it in.
/// </summary>
/// <remarks>
/// <para>
/// <b>The order is the safety.</b> Write a sibling; verify it opens and holds what was planned;
/// move the original into staging; rename the sibling into its place. Nothing is deleted to make
/// room and a failure at any step puts everything back — word for word the invariant
/// <see cref="TransferExecutor"/> keeps for a Replace, because it is the same risk: the thing being
/// displaced is the user's own data.
/// </para>
/// <para>
/// <b>The original goes to staging, not to the bin and not to oblivion.</b> That is what makes the
/// edit undoable, and retiring its undo history entry is the only thing that finally erases it —
/// the same contract a displaced Replace has. An undo sets the <em>edited</em> container aside in
/// turn, under <see cref="EditedMarker"/>, so the edit can be redone.
/// </para>
/// <para>
/// <b>A cancel deletes the sibling and touches nothing else</b>, which is the entire reason for
/// building the new container first: at every moment before the swap, the archive on disk is the
/// one the user started with.
/// </para>
/// </remarks>
public sealed class ArchiveEditExecutor
{
    /// <summary>Marks a container being built, before it is swapped in.</summary>
    public const string RewriteMarker = ".bertbrowser-rewrite-";

    /// <summary>Marks a replaced container while it is still undoable.</summary>
    public const string ReplacedMarker = ".bertbrowser-replaced-";

    /// <summary>Marks an edited container an undo set aside, while it can still be redone.</summary>
    public const string EditedMarker = ".bertbrowser-edited-";

    private readonly IArchiveReader _reader;

    public ArchiveEditExecutor(IArchiveReader reader) => _reader = reader;

    public ArchiveEditOutcome Execute(
        ArchiveEditPlan plan,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null,
        PauseGate? pause = null)
    {
        if (!plan.HasWork) return ArchiveEditOutcome.Nothing(plan.ArchiveFile);

        var archive = plan.ArchiveFile;
        var rewrite = Beside(archive, RewriteMarker);
        var run = new ProgressCoalescer(ct, progress, 0, pause);
        var written = 0;

        try
        {
            written = BuildReplacement(plan, rewrite, run, ct);

            // Verified before anything irreversible happens: if the container we just wrote will
            // not open, the one on disk is still untouched and this costs nothing but a temp file.
            var check = _reader.Read(rewrite, password: null);
            if (!check.Ok)
            {
                TryDelete(rewrite);
                return new ArchiveEditOutcome(
                    archive, null, 0,
                    $"The rewritten archive could not be read back ({check.Error}), so nothing was changed.",
                    false);
            }

            var original = EntryStamp.Of(archive);
            var staged = Beside(archive, ReplacedMarker);
            File.Move(archive, staged);

            try
            {
                File.Move(rewrite, archive);
            }
            catch (Exception)
            {
                // Put the original back before letting the failure out. Leaving the archive absent
                // because the second half of a swap failed is the one outcome this must not have.
                TryRestore(staged, archive);
                throw;
            }

            run.Finished();
            return new ArchiveEditOutcome(archive, staged, written, null, false)
            {
                Original = original,
                Edited = EntryStamp.Of(archive),
            };
        }
        catch (OperationCanceledException)
        {
            TryDelete(rewrite);
            run.Finished();
            return new ArchiveEditOutcome(archive, null, 0, null, Cancelled: true);
        }
        catch (Exception ex) when (IsEditFailure(ex))
        {
            TryDelete(rewrite);
            run.Finished();
            return new ArchiveEditOutcome(archive, null, 0, ex.Message, false);
        }
    }

    /// <summary>
    /// Undoes an edit by swapping the staged original back in. The edited container is set aside
    /// under <see cref="EditedMarker"/> rather than deleted, which is what lets it be redone.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Refused when the container is no longer the one the edit wrote. The undo history can reach an
    /// edit many operations later, and by then something else may have rewritten the file — setting
    /// that aside for an older original would be an undo of somebody else's change.
    /// </para>
    /// <para>
    /// Uncancellable and silent on purpose, the same as the transfer executor's staging moves: a
    /// cancel landing half-way through putting a container back would strand it.
    /// </para>
    /// </remarks>
    public ArchiveEditUndo Undo(ArchiveEditOutcome outcome)
    {
        var archive = outcome.ArchiveFile;
        ArchiveEditUndo Refuse(string why) => new(archive, null, outcome.Original, outcome.Edited, why);

        if (outcome.StagedOriginal is not { } staged) return Refuse("There is nothing to put back.");
        if (!File.Exists(staged)) return Refuse("The replaced archive is no longer there.");
        if (outcome.Edited is { } edited && File.Exists(archive) && !edited.Matches(archive))
            return Refuse($"{Path.GetFileName(archive)} has changed since the edit, so it was left as it is.");

        var aside = Beside(archive, EditedMarker);
        try
        {
            if (File.Exists(archive)) File.Move(archive, aside);
            try
            {
                File.Move(staged, archive);
            }
            catch (Exception)
            {
                TryRestore(aside, archive);
                throw;
            }

            // Stamped afresh rather than carried over: a rename into a name freed seconds ago takes
            // on the old entry's creation time (NTFS tunnelling), so only a stamp taken after the
            // move describes what is really there now.
            return new ArchiveEditUndo(
                archive, File.Exists(aside) ? aside : null, EntryStamp.Of(archive), outcome.Edited, null);
        }
        catch (Exception ex) when (IsEditFailure(ex))
        {
            return Refuse($"The archive could not be put back: {ex.Message}. It is at {staged}.");
        }
    }

    /// <summary>
    /// Takes an undone edit back in: the original is set aside again and the edited container
    /// returns. Refused when the container in place is no longer the original the undo restored.
    /// </summary>
    public ArchiveEditOutcome Redo(ArchiveEditUndo undo)
    {
        var archive = undo.ArchiveFile;
        ArchiveEditOutcome Refuse(string why) =>
            new(archive, null, 0, why, false) { Original = undo.Original, Edited = undo.Edited };

        if (undo.StagedEdited is not { } edited) return Refuse("There is no edit to take back in.");
        if (!File.Exists(edited)) return Refuse("The edited archive is no longer being held.");
        if (undo.Original is { } original && File.Exists(archive) && !original.Matches(archive))
            return Refuse($"{Path.GetFileName(archive)} has changed since the undo, so it was left as it is.");

        var staged = Beside(archive, ReplacedMarker);
        try
        {
            var before = EntryStamp.Of(archive);
            if (File.Exists(archive)) File.Move(archive, staged);
            try
            {
                File.Move(edited, archive);
            }
            catch (Exception)
            {
                TryRestore(staged, archive);
                throw;
            }

            return new ArchiveEditOutcome(archive, File.Exists(staged) ? staged : null, 0, null, false)
            {
                Original = before,
                Edited = EntryStamp.Of(archive),
            };
        }
        catch (Exception ex) when (IsEditFailure(ex))
        {
            return Refuse($"The edit could not be taken back in: {ex.Message}. It is at {edited}.");
        }
    }

    /// <summary>
    /// Erases a staged original, once the edit can no longer be undone — when the undo history
    /// releases it, or the session ends.
    /// </summary>
    public static void CommitStaging(ArchiveEditOutcome outcome)
    {
        if (outcome.StagedOriginal is not { } staged) return;

        // Named the way this class names them, or it is not ours to erase — the guard every
        // recursive delete in this codebase carries, applied to a single file.
        if (!Path.GetFileName(staged).Contains(ReplacedMarker, StringComparison.Ordinal)) return;

        TryDelete(staged);
    }

    /// <summary>Erases a held edited container, once the undone edit can no longer be redone.</summary>
    public static void CommitStaging(ArchiveEditUndo undo)
    {
        if (undo.StagedEdited is not { } edited) return;
        if (!Path.GetFileName(edited).Contains(EditedMarker, StringComparison.Ordinal)) return;

        TryDelete(edited);
    }

    /// <summary>Streams the old container into a new one, applying the edits on the way through.</summary>
    private int BuildReplacement(
        ArchiveEditPlan plan, string rewrite, ProgressCoalescer run, CancellationToken ct)
    {
        var format = ArchiveFormats.Match(Path.GetFileName(plan.ArchiveFile))!;
        var written = 0;

        using (var file = new FileStream(
                   rewrite, FileMode.Create, FileAccess.Write, FileShare.None,
                   bufferSize: 64 * 1024, FileOptions.SequentialScan))
        using (var writer = WriterFactory.Open(file, TypeFor(format), OptionsFor(format)))
        {
            var index = _reader.Read(plan.ArchiveFile, password: null);
            var keep = index.ByPath.Values
                .Where(n => !n.IsDirectory && !plan.Removals.Contains(n.Path))
                .Select(n => n.Path)
                .ToList();

            if (keep.Count > 0)
            {
                _reader.ReadEntries(plan.ArchiveFile, keep, password: null, (entryPath, content, size) =>
                {
                    ct.ThrowIfCancellationRequested();

                    var name = plan.Renames.TryGetValue(entryPath, out var renamed) ? renamed : entryPath;
                    run.BeginItem(Path.GetFileName(name));
                    run.BeginFile(size);

                    // Buffered because the writers want a seekable stream to take a length from,
                    // and an entry stream out of a compressed container is forward-only. Bounded by
                    // the refusals in the planner rather than by hope.
                    using var buffer = Buffered(content, size, ct);
                    writer.Write(name.Replace('\\', '/'), buffer, index.Find(entryPath)?.Modified);

                    run.EndFile();
                    run.EndItem();
                    written++;
                }, ct);
            }

            foreach (var add in plan.Additions)
            {
                ct.ThrowIfCancellationRequested();
                run.BeginItem(Path.GetFileName(add.SourcePath));

                using var source = new FileStream(
                    add.SourcePath, FileMode.Open, FileAccess.Read,
                    FileShare.ReadWrite | FileShare.Delete);

                run.BeginFile(source.Length);
                writer.Write(add.EntryPath.Replace('\\', '/'), source,
                    File.GetLastWriteTime(add.SourcePath));

                run.EndFile();
                run.EndItem();
                written++;
            }
        }

        return written;
    }

    private static MemoryStream Buffered(Stream content, long size, CancellationToken ct)
    {
        var buffer = new MemoryStream(capacity: (int)Math.Clamp(size, 0, 16 << 20));
        var chunk = new byte[64 * 1024];
        int read;
        while ((read = content.Read(chunk, 0, chunk.Length)) > 0)
        {
            ct.ThrowIfCancellationRequested();
            buffer.Write(chunk, 0, read);
        }
        buffer.Position = 0;
        return buffer;
    }

    /// <summary>
    /// A working name beside the archive, so moving between them is a rename rather than a copy.
    /// </summary>
    /// <remarks>
    /// <b>The marker goes before the suffix, not after it.</b> <c>a.zip.bertbrowser-rewrite-1234</c>
    /// is not a name any archive suffix matches, so the reader would refuse to open it — and the
    /// read-back that verifies the rewrite before anything irreversible happens would fail every
    /// time. <c>a.bertbrowser-rewrite-1234.zip</c> is still a zip.
    /// </remarks>
    private static string Beside(string archive, string marker)
    {
        var directory = Path.GetDirectoryName(archive) ?? "";
        var suffix = ArchiveFormats.Match(Path.GetFileName(archive))?.Suffix
                     ?? Path.GetExtension(archive);
        var stem = ExtractRules.StemOf(archive);

        return Path.Combine(directory, stem + marker + Guid.NewGuid().ToString("N")[..8] + suffix);
    }

    private static ArchiveType TypeFor(ArchiveFormat format) =>
        format.Container == ArchiveContainer.Zip ? ArchiveType.Zip : ArchiveType.Tar;

    /// <summary>The compression the container already used, so a rewrite does not silently
    /// change what kind of file it is.</summary>
    private static WriterOptions OptionsFor(ArchiveFormat format) =>
        new(format.Suffix switch
        {
            ".tar" => CompressionType.None,
            ".tar.gz" or ".tgz" => CompressionType.GZip,
            ".tar.bz2" or ".tbz2" => CompressionType.BZip2,
            _ => CompressionType.Deflate,
        });

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception ex) when (IsEditFailure(ex)) { /* best effort */ }
    }

    private static void TryRestore(string staged, string archive)
    {
        try
        {
            if (File.Exists(staged) && !File.Exists(archive)) File.Move(staged, archive);
        }
        catch (Exception ex) when (IsEditFailure(ex)) { /* the message names the staged path */ }
    }

    private static bool IsEditFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException or SharpCompressException;
}
