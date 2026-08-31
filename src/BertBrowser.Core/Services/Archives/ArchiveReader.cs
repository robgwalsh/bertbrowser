using SharpCompress.Archives;
using SharpCompress.Common;
using SharpCompress.Readers;

namespace BertBrowser.Core.Services.Archives;

/// <summary>Reading a container: its entries, and the bytes of one entry.</summary>
/// <remarks>
/// The seam exists for the reason <see cref="NewItem.INewItemProbe"/> does — so the rules above it
/// are testable without a real archive — and for one more: it is what confines the compression
/// library to a single implementation. See <see cref="SharpCompressArchiveReader"/>.
/// </remarks>
public interface IArchiveReader
{
    /// <summary>
    /// Reads a container's directory. <b>Never throws</b>: a damaged, encrypted, oversized or
    /// unreadable archive comes back as an <see cref="ArchiveIndex"/> carrying an
    /// <see cref="ArchiveFailure"/> and a message.
    /// </summary>
    ArchiveIndex Read(string archiveFile, string? password, CancellationToken ct = default);

    /// <summary>
    /// Copies up to <paramref name="maxBytes"/> of one entry into memory. Null when the entry is
    /// missing or unreadable. <b>Bounding the read bounds the decompression</b>, which is what a
    /// zip bomb cannot get around: pulling 1 MB out of a stream that would have produced 10 GB
    /// costs 1 MB.
    /// </summary>
    byte[]? ReadEntry(string archiveFile, string entryPath, long maxBytes, string? password,
        CancellationToken ct = default);

    /// <summary>
    /// Hands each wanted entry's content to <paramref name="onEntry"/>, in one pass over the
    /// container.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>One pass, not one per entry, and that is the whole reason this is not a loop over
    /// <see cref="ReadEntry"/>.</b> Pulling three files out of a solid 7z entry by entry means
    /// decompressing from the start three times; asking for all of them at once costs one.
    /// </para>
    /// <para>
    /// The split of responsibility is deliberate: this owns iterating the archive, and the caller
    /// owns the filesystem. That is what lets the executor keep the conflict rules, the record of
    /// what it created and the cancel-cleanup entirely to itself.
    /// </para>
    /// <para>
    /// Entry paths are normalised keys, as <see cref="ArchiveIndex"/> reports them. Anything not
    /// found is simply never announced — the caller compares against what it asked for.
    /// </para>
    /// </remarks>
    void ReadEntries(
        string archiveFile,
        IReadOnlyCollection<string> entryPaths,
        string? password,
        Action<string, Stream, long> onEntry,
        CancellationToken ct = default);
}

/// <summary>
/// The only class in this repository that names SharpCompress.
/// </summary>
/// <remarks>
/// <para>
/// <b>That confinement is the justification for the dependency.</b> A package reachable from one
/// class is a package that can be replaced; one whose types reach a ViewModel is one you have
/// married. It earns its place where Markdig and PdfPig deliberately do not because there is no
/// alternative at all — <c>System.IO.Compression</c> reads zip and nothing else, and hand-rolling
/// an LZMA or RAR decoder is not a file browser's business.
/// </para>
/// <para>
/// <b>Two APIs, chosen by <see cref="ArchiveFormat.RandomAccess"/>, and the choice is not
/// cosmetic.</b> Opened through the random-access API a <c>.tar.gz</c> comes back as a GZip holding
/// one entry whose key is <c>null</c>, and a <c>.tar.bz2</c> throws <c>InvalidOperationException</c>
/// outright. Through the forward-only reader both come back as a Tar with their real entries. All
/// measured against the library, not inferred from its documentation.
/// </para>
/// <para>
/// <b>What the bytes turn out to be must match what the name claimed.</b> 512,000 zero bytes named
/// <c>archive.zip</c> is a <em>valid empty tar</em> — a tar's end-of-archive marker is zero blocks —
/// so without this check the harness's own filler fixture browses as an empty folder rather than
/// reporting a file that is not what it says it is. That fixture is listed by three existing
/// scripts, so this is the ordinary case rather than an exotic one.
/// </para>
/// <para>
/// <b>Nothing is held open.</b> Every read opens with <c>FileShare.ReadWrite | Delete</c>, takes
/// what it needs and closes — the preview pane's rule, and it matters more here because a browsing
/// session lasts minutes rather than milliseconds. What a held handle would block is this app's own
/// rename, move and delete executors: browsing into a zip must not stop you deleting it.
/// </para>
/// <para>
/// <b>The catch is deliberately wide.</b> A malformed archive is attacker-controlled input handed
/// to third-party decoders, and they throw whatever they throw — <c>InvalidOperationException</c>,
/// index and overflow exceptions included. <c>ArchiveContents.Failed</c>'s existing contract, that
/// a damaged archive is a message rather than a throw, now has to hold up a whole browsing surface
/// instead of one panel.
/// </para>
/// <para>
/// <b>Synchronous on purpose</b>, like <see cref="IFileSystemService"/>: callers own their
/// <c>Task.Run</c>. SharpCompress's own async extraction is <em>slower</em> for 7z, because the
/// LZMA decoder's dictionary state assumes uninterrupted sequential processing and an async path
/// rebuilds it per file.
/// </para>
/// </remarks>
public sealed class SharpCompressArchiveReader : IArchiveReader
{
    private readonly int _maxEntries;

    public SharpCompressArchiveReader() : this(ArchiveIndex.MaxEntries) { }

    internal SharpCompressArchiveReader(int maxEntries) => _maxEntries = maxEntries;

    public ArchiveIndex Read(string archiveFile, string? password, CancellationToken ct = default)
    {
        var format = ArchiveFormats.Match(Path.GetFileName(archiveFile));
        if (format is null)
            return ArchiveIndex.Failed(ArchiveFailure.Damaged, "Not a readable archive, or damaged.");

        try
        {
            return format.RandomAccess
                ? ReadAddressable(archiveFile, format, password, ct)
                : ReadSequential(archiveFile, format, password, ct);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        // SharpCompress.Common.CryptographicException, NOT the framework type of the same name.
        // They shadow each other, and a bare catch in a file carrying the System.Security.
        // Cryptography using would compile, catch the wrong one, and let a wrong password escape
        // unhandled out of a Task.Run. Fully qualified here and everywhere else.
        catch (SharpCompress.Common.CryptographicException)
        {
            return ArchiveIndex.Failed(
                ArchiveFailure.PasswordRequired,
                password is null
                    ? "This archive is protected. A password is needed to read it."
                    : "That password did not work.");
        }
        catch (FileNotFoundException)
        {
            return ArchiveIndex.Failed(ArchiveFailure.Unreadable, "The archive is no longer there.");
        }
        catch (DirectoryNotFoundException)
        {
            return ArchiveIndex.Failed(ArchiveFailure.Unreadable, "The archive is no longer there.");
        }
        catch (UnauthorizedAccessException)
        {
            return ArchiveIndex.Failed(ArchiveFailure.Unreadable, "The archive could not be opened.");
        }
        catch (Exception ex) when (IsMalformedArchive(ex))
        {
            // A password was given and the container still would not parse. Decrypting a header
            // with the wrong key produces garbage rather than a crypto error, so this arrives here
            // rather than in the catch above — and reporting it as "damaged" would dead-end the one
            // user who can fix it. With a key in hand, a wrong key is overwhelmingly the likelier
            // reading of bytes that will not parse; without one, damaged is right.
            if (password is not null)
                return ArchiveIndex.Failed(
                    ArchiveFailure.PasswordRequired, "That password did not work.");

            return ArchiveIndex.Failed(ArchiveFailure.Damaged, "Not a readable archive, or damaged.");
        }
    }

    private ArchiveIndex ReadAddressable(
        string archiveFile, ArchiveFormat format, string? password, CancellationToken ct)
    {
        using var stream = OpenShared(archiveFile);
        using var archive = ArchiveFactory.Open(stream, OptionsFor(password));

        if (Map(archive.Type) != format.Container) return Mismatched();

        // Encryption is reported per entry, not on the archive — 1.0.0 has no archive-level flag.
        // Deriving it from the entries is the better source anyway: a zip may hold two encrypted
        // files among a hundred plain ones, and that is exactly the case where the listing is shown
        // in full with a lock on the rows that need a password.
        var raw = new List<RawArchiveEntry>();
        var anyEncrypted = false;

        foreach (var entry in archive.Entries)
        {
            ct.ThrowIfCancellationRequested();
            anyEncrypted |= entry.IsEncrypted;
            raw.Add(new RawArchiveEntry(
                entry.Key, entry.Size, entry.CompressedSize, entry.LastModifiedTime,
                entry.IsDirectory, entry.IsEncrypted, entry.LinkTarget));

            if (raw.Count > _maxEntries) break;
        }

        return ArchiveIndexBuilder.Build(
            raw,
            new ArchiveCapabilities(archive.IsSolid, anyEncrypted, archive.IsComplete),
            _maxEntries);
    }

    private ArchiveIndex ReadSequential(
        string archiveFile, ArchiveFormat format, string? password, CancellationToken ct)
    {
        using var stream = OpenShared(archiveFile);
        using var reader = ReaderFactory.Open(stream, OptionsFor(password));

        if (Map(reader.ArchiveType) != format.Container) return Mismatched();

        var raw = new List<RawArchiveEntry>();
        var anyEncrypted = false;

        while (reader.MoveToNextEntry())
        {
            ct.ThrowIfCancellationRequested();
            var entry = reader.Entry;
            anyEncrypted |= entry.IsEncrypted;
            raw.Add(new RawArchiveEntry(
                entry.Key, entry.Size, entry.CompressedSize, entry.LastModifiedTime,
                entry.IsDirectory, entry.IsEncrypted, entry.LinkTarget));

            if (raw.Count > _maxEntries) break;
        }

        // Nothing here is addressable, whatever the container's own opinion of solidity.
        return ArchiveIndexBuilder.Build(
            raw,
            new ArchiveCapabilities(SequentialOnly: true, anyEncrypted, IsComplete: true),
            _maxEntries);
    }

    public byte[]? ReadEntry(
        string archiveFile, string entryPath, long maxBytes, string? password,
        CancellationToken ct = default)
    {
        if (maxBytes <= 0) return null;

        var format = ArchiveFormats.Match(Path.GetFileName(archiveFile));
        if (format is null) return null;

        try
        {
            using var stream = OpenShared(archiveFile);

            if (format.RandomAccess)
            {
                using var archive = ArchiveFactory.Open(stream, OptionsFor(password));
                if (Map(archive.Type) != format.Container) return null;

                foreach (var entry in archive.Entries)
                {
                    ct.ThrowIfCancellationRequested();
                    if (entry.IsDirectory || !Matches(entry.Key, entryPath)) continue;
                    using var entryStream = entry.OpenEntryStream();
                    return CopyBounded(entryStream, maxBytes, ct);
                }
                return null;
            }

            using var reader = ReaderFactory.Open(stream, OptionsFor(password));
            if (Map(reader.ArchiveType) != format.Container) return null;

            while (reader.MoveToNextEntry())
            {
                ct.ThrowIfCancellationRequested();
                if (reader.Entry.IsDirectory || !Matches(reader.Entry.Key, entryPath)) continue;
                using var entryStream = reader.OpenEntryStream();
                return CopyBounded(entryStream, maxBytes, ct);
            }
            return null;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (SharpCompress.Common.CryptographicException)
        {
            return null;
        }
        catch (Exception ex) when (IsMalformedArchive(ex) ||
                                   ex is FileNotFoundException or DirectoryNotFoundException
                                       or UnauthorizedAccessException)
        {
            return null;
        }
    }

    public void ReadEntries(
        string archiveFile,
        IReadOnlyCollection<string> entryPaths,
        string? password,
        Action<string, Stream, long> onEntry,
        CancellationToken ct = default)
    {
        if (entryPaths.Count == 0) return;

        var format = ArchiveFormats.Match(Path.GetFileName(archiveFile))
            ?? throw new IOException("Not a readable archive, or damaged.");

        var wanted = new HashSet<string>(entryPaths, StringComparer.OrdinalIgnoreCase);
        var seen = 0;

        using var stream = OpenShared(archiveFile);

        if (format.RandomAccess)
        {
            using var archive = ArchiveFactory.Open(stream, OptionsFor(password));
            if (Map(archive.Type) != format.Container)
                throw new IOException("Not a readable archive, or damaged.");

            foreach (var entry in archive.Entries)
            {
                ct.ThrowIfCancellationRequested();
                if (entry.IsDirectory) continue;
                if (ArchiveIndexBuilder.Normalize(entry.Key) is not { } key || !wanted.Contains(key))
                    continue;

                using var content = entry.OpenEntryStream();
                onEntry(key, content, entry.Size);
                if (++seen == wanted.Count) return;
            }
            return;
        }

        using var reader = ReaderFactory.Open(stream, OptionsFor(password));
        if (Map(reader.ArchiveType) != format.Container)
            throw new IOException("Not a readable archive, or damaged.");

        while (reader.MoveToNextEntry())
        {
            ct.ThrowIfCancellationRequested();
            if (reader.Entry.IsDirectory) continue;
            if (ArchiveIndexBuilder.Normalize(reader.Entry.Key) is not { } key || !wanted.Contains(key))
                continue;

            using var content = reader.OpenEntryStream();
            onEntry(key, content, reader.Entry.Size);
            if (++seen == wanted.Count) return;
        }
    }

    private static bool Matches(string? key, string entryPath) =>
        ArchiveIndexBuilder.Normalize(key) is { } normalized &&
        normalized.Equals(entryPath, StringComparison.OrdinalIgnoreCase);

    private static ArchiveIndex Mismatched() => ArchiveIndex.Failed(
        ArchiveFailure.Damaged, "Not a readable archive, or damaged.");

    /// <summary>
    /// The library's detected type in this app's own words. Anything not listed here is something
    /// no suffix in <see cref="ArchiveFormats"/> claims, so it can never match and is refused.
    /// </summary>
    private static ArchiveContainer? Map(ArchiveType type) => type switch
    {
        ArchiveType.Zip => ArchiveContainer.Zip,
        ArchiveType.SevenZip => ArchiveContainer.SevenZip,
        ArchiveType.Rar => ArchiveContainer.Rar,
        ArchiveType.Tar => ArchiveContainer.Tar,
        ArchiveType.GZip => ArchiveContainer.GZip,
        _ => null,
    };

    /// <summary>
    /// The sharing flags are the rule, not a detail: this app's own executors are what a held
    /// handle would block.
    /// </summary>
    private static FileStream OpenShared(string path) =>
        new(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);

    private static ReaderOptions OptionsFor(string? password) =>
        new() { LeaveStreamOpen = true, Password = password };

    private static byte[] CopyBounded(Stream source, long maxBytes, CancellationToken ct)
    {
        var buffer = new byte[81920];
        using var sink = new MemoryStream();
        long total = 0;

        while (total < maxBytes)
        {
            ct.ThrowIfCancellationRequested();
            var want = (int)Math.Min(buffer.Length, maxBytes - total);
            var read = source.Read(buffer, 0, want);
            if (read <= 0) break;
            sink.Write(buffer, 0, read);
            total += read;
        }
        return sink.ToArray();
    }

    /// <summary>
    /// Everything a decoder can throw at bytes that are not what they claim to be. Wide on purpose
    /// — see the class remarks. <c>Exception</c> itself is not caught: a real bug should still
    /// surface as one.
    /// </summary>
    private static bool IsMalformedArchive(Exception ex) =>
        ex is SharpCompressException
            or IOException
            or InvalidDataException
            or EndOfStreamException
            or InvalidOperationException
            or NotSupportedException
            or IndexOutOfRangeException
            or ArgumentOutOfRangeException
            or ArgumentException
            or OverflowException
            or FormatException
            or ObjectDisposedException;
}
