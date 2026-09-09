using System.Buffers;
using BertBrowser.Core.Interop;
using BertBrowser.Core.Services.Checksums;

namespace BertBrowser.Core.Services.Duplicates;

/// <summary>Which file on disk a name actually refers to, when it answers to more than one.</summary>
public readonly record struct FileIdentity(uint VolumeSerialNumber, ulong FileIndex);

/// <summary>
/// What hashing one file learned about it.
/// </summary>
/// <param name="BytesRead">
/// What was actually read, which is not necessarily what the index said the file was: the row may
/// be stale, and a file that shrank between the shortlist and the hash must not be compared against
/// one that did not.
/// </param>
/// <param name="Identity">
/// Set only when the file carries more than one name. Null is the common case and means "this is
/// its own file"; the scanner then does no identity bookkeeping at all.
/// </param>
public sealed record FileFingerprint(string Hash, long BytesRead, FileIdentity? Identity);

/// <summary>
/// Hashing the bytes of one file, with progress and interruption.
/// </summary>
/// <remarks>
/// This is a seam for the same reason <see cref="Transfer.IFileCopier"/> is one: it is what lets
/// <c>DuplicateScannerTests</c> drive a cancel that lands <em>in the middle of a file</em>
/// deterministically and in milliseconds, and lets the grouping rules be tested against contrived
/// collisions without writing files that really collide.
/// </remarks>
public interface IFileHasher
{
    /// <summary>
    /// Hashes at most <paramref name="maxBytes"/> of <paramref name="path"/> — the whole file when
    /// that is zero or negative. <paramref name="progress"/> is called with each chunk's byte count
    /// as it lands, never a running total, so a caller hashing several files at once can add them
    /// up itself.
    /// </summary>
    /// <returns>
    /// Null when the file cannot be read, or must not be. That is not an error: one unreadable
    /// candidate marks the scan incomplete and the rest carry on, exactly as one item's failure
    /// never costs the others anywhere else in this app.
    /// </returns>
    /// <remarks>
    /// A cancelled hash throws <see cref="OperationCanceledException"/> rather than returning null.
    /// The difference is load-bearing: one is this file having a problem, the other is the whole run
    /// stopping, and conflating them would let a cancel look like a disk full of unreadable files.
    /// </remarks>
    FileFingerprint? Hash(string path, long maxBytes, Action<long>? progress, CancellationToken ct);
}


/// <summary>Real-filesystem <see cref="IFileHasher"/> and <see cref="IFileDigester"/>.</summary>
/// <remarks>
/// <para>
/// <b>SHA-256, from the BCL</b>, for the duplicate finder. It is hardware-accelerated on every
/// machine this app runs on, it needs no package, and its equality is strong enough to act on —
/// which matters, because what the user does with the answer is delete files. A faster
/// non-cryptographic hash would have to be followed by a byte-for-byte compare to be safe, and that
/// costs more than the stronger hash did. The checksum tool's choice of algorithm reaches
/// <see cref="IFileDigester"/> and can never reach <see cref="Hash"/>: that is why they are two
/// interfaces rather than one with a parameter.
/// </para>
/// <para>
/// <b>One read loop.</b> Both seams run <see cref="ReadFile"/>, so the sharing rules, the
/// placeholder and reparse refusals, the buffer size and the per-chunk progress contract exist once.
/// A regression in any of them shows up in both features at the same time, which is the point —
/// <c>FileSystemFileHasherTests</c> is what holds them down and it did not change when the digester
/// arrived.
/// </para>
/// <para>
/// <b>Nothing is held open.</b> Every read shares <see cref="FileShare.ReadWrite"/> and
/// <see cref="FileShare.Delete"/> via <see cref="ReadOnlyFile"/>, the rule the preview pane already
/// follows: hashing must never block this app's own rename, move and delete executors, which is what
/// a plain read lock would do to the folder the user is standing in.
/// </para>
/// </remarks>
public sealed class FileSystemFileHasher : IFileHasher, IFileDigester
{
    /// <summary>Big enough that a large file is a few hundred reads, small enough not to matter.</summary>
    private const int BufferBytes = 1024 * 1024;

    public FileFingerprint? Hash(string path, long maxBytes, Action<long>? progress, CancellationToken ct)
    {
        using var digest = DigestSinks.Create(ChecksumAlgorithm.Sha256);

        if (ReadFile(path, maxBytes, [digest], wantIdentity: true, progress, ct) is not { } outcome)
            return null;

        return new FileFingerprint(digest.Finish(), outcome.BytesRead, outcome.Identity);
    }

    public FileDigests? Digest(
        string path,
        IReadOnlyList<ChecksumAlgorithm> algorithms,
        Action<long>? progress,
        CancellationToken ct)
    {
        var wanted = ChecksumAlgorithms.Normalise(algorithms);

        // Asking for nothing is an empty answer, not a failure — a UI with every box unticked has
        // nothing to say about a file, which is different from being unable to read it.
        if (wanted.Count == 0)
            return new FileDigests(new Dictionary<ChecksumAlgorithm, string>(), 0);

        var sinks = new IDigestSink[wanted.Count];
        try
        {
            for (var i = 0; i < wanted.Count; i++) sinks[i] = DigestSinks.Create(wanted[i]);

            if (ReadFile(path, maxBytes: 0, sinks, wantIdentity: false, progress, ct) is not { } outcome)
                return null;

            var digests = new Dictionary<ChecksumAlgorithm, string>(wanted.Count);
            for (var i = 0; i < wanted.Count; i++) digests[wanted[i]] = sinks[i].Finish();

            return new FileDigests(digests, outcome.BytesRead);
        }
        finally
        {
            foreach (var sink in sinks) sink?.Dispose();
        }
    }

    /// <summary>What one pass over a file learned, beyond what the digests themselves hold.</summary>
    private readonly record struct ReadOutcome(long BytesRead, FileIdentity? Identity);

    /// <summary>
    /// Reads a file once, feeding every byte to every sink.
    /// </summary>
    /// <remarks>
    /// The shared half of both public methods. Note that <paramref name="progress"/> is invoked once
    /// per chunk regardless of how many sinks there are: the caller is being told how much of the
    /// file has been read, not how much hashing happened.
    /// </remarks>
    private static ReadOutcome? ReadFile(
        string path,
        long maxBytes,
        IReadOnlyList<IDigestSink> sinks,
        bool wantIdentity,
        Action<long>? progress,
        CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        // A reparse point is the one entry it is, not the file it points at — the same reading
        // DeleteSurveyor takes. Following it would hash the target a second time under a name
        // that is not really a copy of anything.
        if (ReadOnlyFile.TryOpen(path) is not { } stream) return null;

        using (stream)
        {
            FileIdentity? identity = null;
            if (wantIdentity &&
                FileIdentityNative.TryRead(stream.SafeFileHandle, out var links, out var id) && links > 1)
                identity = new FileIdentity(id.Volume, id.Index);

            var buffer = ArrayPool<byte>.Shared.Rent(BufferBytes);
            try
            {
                var limit = maxBytes > 0 ? maxBytes : long.MaxValue;
                var read = 0L;

                while (read < limit)
                {
                    ct.ThrowIfCancellationRequested();

                    var want = (int)Math.Min(buffer.Length, limit - read);
                    int got;
                    try
                    {
                        got = stream.Read(buffer, 0, want);
                    }
                    catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
                    {
                        // Failing part-way is still a failure: a hash of the first half of a file
                        // would compare equal to nothing and unequal to everything, silently.
                        return null;
                    }

                    if (got == 0) break;

                    var chunk = buffer.AsSpan(0, got);
                    foreach (var sink in sinks) sink.Append(chunk);

                    read += got;
                    progress?.Invoke(got);
                }

                return new ReadOutcome(read, identity);
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
            }
        }
    }
}
