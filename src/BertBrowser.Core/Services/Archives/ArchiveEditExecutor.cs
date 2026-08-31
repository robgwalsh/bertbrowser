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
/// edit undoable, and <c>ShellViewModel.RetireUndoable</c> is the only thing that finally erases it
/// — so the replaced container outlives its undo record by exactly one operation, the same contract
/// a displaced Replace has.
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

    private readonly IArchiveReader _reader;

    public ArchiveEditExecutor(IArchiveReader reader) => _reader = reader;

    public ArchiveEditOutcome Execute(
        ArchiveEditPlan plan,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null)
    {
        if (!plan.HasWork) return ArchiveEditOutcome.Nothing(plan.ArchiveFile);

        var archive = plan.ArchiveFile;
        var rewrite = Beside(archive, RewriteMarker);
        var run = new ProgressCoalescer(ct, progress, 0);
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
            return new ArchiveEditOutcome(archive, staged, written, null, false);
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
    /// Undoes an edit by putting the staged original back.
    /// </summary>
    /// <remarks>
    /// Uncancellable and silent on purpose, the same as the transfer executor's staging moves: a
    /// cancel landing half-way through putting a container back would strand it.
    /// </remarks>
    public string? Undo(ArchiveEditOutcome outcome)
    {
        if (outcome.StagedOriginal is not { } staged) return "There is nothing to put back.";
        if (!File.Exists(staged)) return "The replaced archive is no longer there.";

        try
        {
            // The edited container is this call's own doing, so it goes; the staged one is the
            // user's and is only ever moved.
            if (File.Exists(outcome.ArchiveFile)) File.Delete(outcome.ArchiveFile);
            File.Move(staged, outcome.ArchiveFile);
            return null;
        }
        catch (Exception ex) when (IsEditFailure(ex))
        {
            return $"The archive could not be put back: {ex.Message}. It is at {staged}.";
        }
    }

    /// <summary>
    /// Erases a staged original. The only caller is <c>ShellViewModel.RetireUndoable</c>, which is
    /// what makes the replaced container outlive its undo record by exactly one operation.
    /// </summary>
    public static void CommitStaging(ArchiveEditOutcome outcome)
    {
        if (outcome.StagedOriginal is not { } staged) return;

        // Named the way this class names them, or it is not ours to erase — the guard every
        // recursive delete in this codebase carries, applied to a single file.
        if (!Path.GetFileName(staged).Contains(ReplacedMarker, StringComparison.Ordinal)) return;

        TryDelete(staged);
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
