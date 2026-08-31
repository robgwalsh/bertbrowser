using BertBrowser.Core.Services.Transfer;
using SharpCompress.Common;
using SharpCompress.Writers;

namespace BertBrowser.Core.Services.Archives;

/// <summary>One file that is going into a new archive, and the name it will have inside it.</summary>
public sealed record ArchiveSource(string Path, string EntryName, long SizeBytes);

public sealed record CreateArchiveOutcome(
    string ArchivePath,
    int FilesWritten,
    long BytesRead,
    IReadOnlyList<string> Failed,
    bool Cancelled);

/// <summary>
/// Produces a new container from files on disk.
/// </summary>
/// <remarks>
/// <para>
/// <b>Over this app's own walk, not the library's.</b> SharpCompress will happily add a whole
/// directory itself, but then the browse setting would not decide about hidden files, reparse
/// points would be followed rather than skipped, and a cancel could only land between whole
/// directories. Walking it here costs a few lines and keeps all three.
/// </para>
/// <para>
/// <b>A cancelled create leaves nothing.</b> It writes to <c>&lt;target&gt;.bertbrowser-partial</c>
/// and renames on success — one rename apart from the staging idea the transfer and delete
/// executors use. Writing straight to <c>archive.zip</c> and cancelling would leave a truncated
/// file under exactly the name every other tool on the machine will try to open.
/// </para>
/// <para>
/// Progress rides <see cref="ProgressCoalescer"/>, so the status strip and the detail window need no
/// idea this is not an ordinary transfer.
/// </para>
/// </remarks>
public sealed class ArchiveCreator
{
    /// <summary>The suffix a half-written archive carries until it is finished.</summary>
    public const string PartialSuffix = ".bertbrowser-partial";

    public CreateArchiveOutcome Create(
        string archivePath,
        ArchiveWriteFormat format,
        CompressionLevel level,
        IReadOnlyList<ArchiveSource> sources,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null)
    {
        var partial = archivePath + PartialSuffix;
        var run = new ProgressCoalescer(ct, progress, sources.Count);
        var failed = new List<string>();
        var written = 0;
        long read = 0;
        var cancelled = false;

        try
        {
            var parent = Path.GetDirectoryName(archivePath);
            if (parent is { Length: > 0 }) Directory.CreateDirectory(parent);

            using (var file = new FileStream(
                       partial, FileMode.Create, FileAccess.Write, FileShare.None,
                       bufferSize: 64 * 1024, FileOptions.SequentialScan))
            using (var writer = WriterFactory.Open(file, TypeFor(format), OptionsFor(format, level)))
            {
                foreach (var source in sources)
                {
                    ct.ThrowIfCancellationRequested();
                    run.BeginItem(Path.GetFileName(source.Path));
                    run.BeginFile(source.SizeBytes);

                    try
                    {
                        using var content = new FileStream(
                            source.Path, FileMode.Open, FileAccess.Read,
                            FileShare.ReadWrite | FileShare.Delete,
                            bufferSize: 64 * 1024, FileOptions.SequentialScan);

                        // Wrapped so the bytes are counted and the cancel lands *inside* a file
                        // rather than only between them — a single large file is the case that
                        // matters, and it is the one an item-level cancel handles worst.
                        using var counted = new CountingStream(content, run, ct);
                        writer.Write(source.EntryName, counted, File.GetLastWriteTime(source.Path));

                        read += counted.BytesRead;
                        written++;
                    }
                    catch (OperationCanceledException)
                    {
                        throw;
                    }
                    catch (Exception ex) when (IsCreateFailure(ex))
                    {
                        failed.Add($"{Path.GetFileName(source.Path)}: {ex.Message}");
                    }

                    run.EndFile();
                    run.EndItem();
                }
            }

            // Only now does the name everyone recognises come into being.
            File.Move(partial, archivePath, overwrite: false);
        }
        catch (OperationCanceledException)
        {
            cancelled = true;
            TryDelete(partial);
        }
        catch (Exception ex) when (IsCreateFailure(ex))
        {
            failed.Add(ex.Message);
            TryDelete(partial);
        }

        run.Finished();
        return new CreateArchiveOutcome(archivePath, written, read, failed, cancelled);
    }

    private static ArchiveType TypeFor(ArchiveWriteFormat format) => format switch
    {
        ArchiveWriteFormat.Zip => ArchiveType.Zip,
        _ => ArchiveType.Tar,
    };

    /// <summary>
    /// One place the option types are chosen, because they genuinely differ per format and a switch
    /// inside the dialog would drift from the writer.
    /// </summary>
    private static WriterOptions OptionsFor(ArchiveWriteFormat format, CompressionLevel level) =>
        new(format switch
        {
            // Store is the right answer for a folder of JPEGs, and worth offering rather than
            // spending minutes of CPU to save nothing.
            ArchiveWriteFormat.Zip => level == CompressionLevel.Store
                ? CompressionType.None
                : CompressionType.Deflate,
            ArchiveWriteFormat.Tar => CompressionType.None,
            ArchiveWriteFormat.TarGz => CompressionType.GZip,
            ArchiveWriteFormat.TarBz2 => CompressionType.BZip2,
            _ => CompressionType.Deflate,
        });

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception ex) when (IsCreateFailure(ex)) { /* best effort */ }
    }

    private static bool IsCreateFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException or SharpCompressException;

    /// <summary>
    /// Reports what the writer pulls through and lets a cancel land between chunks, so a single
    /// multi-gigabyte file does not become an uninterruptible stretch.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Seeking is delegated, not refused.</b> The tar writer has to know an entry's length
    /// before it writes the header, and asks the stream — a wrapper claiming
    /// <c>CanSeek == false</c> gets "Seekable stream is required if no size is given" and every
    /// tar entry fails. The stream underneath is a file and is perfectly seekable; hiding that
    /// bought nothing.
    /// </para>
    /// <para>
    /// Which is also why progress is read from the position rather than accumulated: a writer that
    /// seeks back and re-reads would otherwise count those bytes twice and drive the total past
    /// the size.
    /// </para>
    /// </remarks>
    private sealed class CountingStream(Stream inner, ProgressCoalescer run, CancellationToken ct)
        : Stream
    {
        public long BytesRead => inner.Position;

        public override int Read(byte[] buffer, int offset, int count)
        {
            ct.ThrowIfCancellationRequested();
            var n = inner.Read(buffer, offset, count);
            if (n > 0) run.FileProgress(inner.Position, Math.Max(inner.Length, inner.Position));
            return n;
        }

        public override bool CanRead => true;
        public override bool CanSeek => inner.CanSeek;
        public override bool CanWrite => false;
        public override long Length => inner.Length;
        public override long Position
        {
            get => inner.Position;
            set => inner.Position = value;
        }
        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => inner.Seek(offset, origin);
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }
}
