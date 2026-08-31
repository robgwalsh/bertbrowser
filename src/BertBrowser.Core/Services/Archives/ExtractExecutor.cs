using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Carries out an <see cref="ExtractPlan"/>.
/// </summary>
/// <remarks>
/// <para>
/// <b>A cancelled extract adds nothing</b>, which is the cancelled-copy rule — but the mechanism
/// has to be stricter than a copy's, and that is the one thing to get right in this file. A copy
/// owns its whole destination tree, so <c>DirectoryRemoval.RemoveTree</c> can clear it. An extract
/// routinely lands <em>into a folder the user already has files in</em>, so removing the tree would
/// delete their work. Instead this records exactly what it created, in creation order, and on a
/// cancel removes only those, in reverse, and only while still empty. A skipped conflict was never
/// created and is never removed.
/// </para>
/// <para>
/// <b>One pass over the container</b>, through <see cref="IArchiveReader.ReadEntries"/>: a solid
/// archive is decompressed once for the whole selection rather than once per file.
/// </para>
/// <para>
/// Progress goes through <see cref="ProgressCoalescer"/>, the same throttle and byte accounting
/// <see cref="TransferExecutor"/> uses, so the status strip and the detail window need no idea that
/// this is not an ordinary transfer.
/// </para>
/// </remarks>
public sealed class ExtractExecutor
{
    private readonly IArchiveReader _reader;
    private readonly IExtractProbe _probe;

    public ExtractExecutor(IArchiveReader reader, IExtractProbe probe)
    {
        _reader = reader;
        _probe = probe;
    }

    public ExtractExecutor(IArchiveReader reader) : this(reader, new FileSystemExtractProbe()) { }

    public ExtractOutcome Execute(
        ExtractPlan plan,
        string? password = null,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null)
    {
        if (!plan.HasWork) return ExtractOutcome.Nothing;

        var files = plan.Items.Where(i => !i.IsDirectory).ToList();
        var run = new ProgressCoalescer(ct, progress, files.Count);

        // Everything this call brings into being, in the order it did, so a cancel can undo exactly
        // that much and nothing of what was already there.
        var created = new List<string>();
        var createdFiles = new List<string>();
        var failed = new List<FailedExtraction>();
        var written = 0;
        long bytes = 0;
        var cancelled = false;

        try
        {
            foreach (var dir in plan.Items.Where(i => i.IsDirectory))
            {
                ct.ThrowIfCancellationRequested();
                CreateDirectories(dir.DestinationPath, created);
            }

            var byEntry = files.ToDictionary(f => f.EntryPath, StringComparer.OrdinalIgnoreCase);

            _reader.ReadEntries(plan.ArchiveFile, byEntry.Keys, password, (entryPath, content, size) =>
            {
                if (!byEntry.TryGetValue(entryPath, out var item)) return;

                run.BeginItem(Path.GetFileName(item.DestinationPath));

                // Re-checked against live disk, because the plan was built while a dialog was open.
                // FileMode.CreateNew rather than a check-then-create: it throws on a taken path,
                // which closes the window between the two.
                var parent = Path.GetDirectoryName(item.DestinationPath);
                if (parent is { Length: > 0 }) CreateDirectories(parent, created);

                run.BeginFile(size);
                try
                {
                    var copied = WriteEntry(content, item.DestinationPath, size, run);
                    createdFiles.Add(item.DestinationPath);
                    created.Add(item.DestinationPath);
                    bytes += copied;
                    written++;
                }
                catch (OperationCanceledException)
                {
                    // The partial file is this call's own doing, so it goes back — but the entries
                    // already finished stay, exactly as a cancelled transfer keeps what got across.
                    TryDelete(item.DestinationPath);
                    throw;
                }
                catch (Exception ex) when (IsExtractFailure(ex))
                {
                    TryDelete(item.DestinationPath);
                    failed.Add(new FailedExtraction(entryPath, ex.Message));
                }
                run.EndFile();
                run.EndItem();
            }, ct);
        }
        catch (OperationCanceledException)
        {
            cancelled = true;
            RemoveWhatThisCallCreated(created, createdFiles);
        }
        catch (Exception ex) when (IsExtractFailure(ex))
        {
            // The container itself failed part-way — a truncated stream, a decoder giving up. What
            // reached disk is real and stays; the outcome says the rest did not.
            failed.Add(new FailedExtraction(plan.ArchiveFile, ex.Message));
        }

        run.Finished();
        return new ExtractOutcome(written, bytes, failed, cancelled);
    }

    /// <summary>
    /// Copies one entry to disk in bounded chunks, reporting bytes and honouring a cancel between
    /// them — the shape <see cref="IFileCopier"/> has, without a source path for the OS to copy from.
    /// </summary>
    private static long WriteEntry(Stream content, string destination, long size, ProgressCoalescer run)
    {
        using var file = new FileStream(
            destination, FileMode.CreateNew, FileAccess.Write, FileShare.None,
            bufferSize: 64 * 1024, FileOptions.SequentialScan);

        var buffer = new byte[64 * 1024];
        long total = 0;
        int read;
        while ((read = content.Read(buffer, 0, buffer.Length)) > 0)
        {
            run.Token.ThrowIfCancellationRequested();
            file.Write(buffer, 0, read);
            total += read;
            run.FileProgress(total, Math.Max(size, total));
        }
        return total;
    }

    /// <summary>
    /// Creates a directory and any missing ancestors, recording each one this call actually brought
    /// into being. An ancestor that was already there is not recorded, so a cancel never removes a
    /// folder the user had.
    /// </summary>
    private void CreateDirectories(string path, List<string> created)
    {
        if (_probe.DirectoryExists(path)) return;

        var stack = new Stack<string>();
        var current = path;
        while (current.Length > 0 && !_probe.DirectoryExists(current))
        {
            stack.Push(current);
            var parent = Path.GetDirectoryName(current);
            if (parent is null || parent == current) break;
            current = parent;
        }

        while (stack.Count > 0)
        {
            var next = stack.Pop();
            if (Directory.Exists(next)) continue;
            Directory.CreateDirectory(next);
            created.Add(next);
        }
    }

    /// <summary>
    /// Undoes exactly this call's additions, newest first. Files it wrote go; directories go only
    /// if still empty, which is what keeps a folder the user had — or had put something else into
    /// while this ran — from being removed.
    /// </summary>
    private static void RemoveWhatThisCallCreated(List<string> created, List<string> createdFiles)
    {
        var fileSet = new HashSet<string>(createdFiles, StringComparer.OrdinalIgnoreCase);

        for (var i = created.Count - 1; i >= 0; i--)
        {
            var path = created[i];
            if (fileSet.Contains(path)) { TryDelete(path); continue; }

            try
            {
                // Never recursive: this must not be able to reach anything it did not put there.
                if (Directory.Exists(path) && !Directory.EnumerateFileSystemEntries(path).Any())
                    Directory.Delete(path, recursive: false);
            }
            catch (Exception ex) when (IsExtractFailure(ex)) { /* leave it rather than force it */ }
        }
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception ex) when (IsExtractFailure(ex)) { /* best effort */ }
    }

    /// <summary>Errors that mean "this item failed" rather than "the program is broken".</summary>
    private static bool IsExtractFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException
            or SharpCompress.Common.SharpCompressException
            or SharpCompress.Common.CryptographicException;
}
