using System.Buffers;
using System.Security.Cryptography;
using BertBrowser.Core.Interop;
using BertBrowser.Core.Services.Preview;

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

/// <summary>Real-filesystem <see cref="IFileHasher"/>.</summary>
/// <remarks>
/// <para>
/// <b>SHA-256, from the BCL.</b> It is hardware-accelerated on every machine this app runs on, it
/// needs no package, and its equality is strong enough to act on — which matters, because what the
/// user does with the answer is delete files. A faster non-cryptographic hash would have to be
/// followed by a byte-for-byte compare to be safe, and that costs more than the stronger hash did.
/// </para>
/// <para>
/// <b>Nothing is held open.</b> Every read shares <see cref="FileShare.ReadWrite"/> and
/// <see cref="FileShare.Delete"/>, the rule the preview pane already follows: hashing must never
/// block this app's own rename, move and delete executors, which is what a plain read lock would do
/// to the folder the user is standing in.
/// </para>
/// </remarks>
public sealed class FileSystemFileHasher : IFileHasher
{
    /// <summary>Big enough that a large file is a few hundred reads, small enough not to matter.</summary>
    private const int BufferBytes = 1024 * 1024;

    /// <summary>
    /// A cloud file whose bytes are not on this machine, per the two attributes .NET does not name
    /// (see <see cref="PreviewClassifier"/>, which refuses the same set for the same reason).
    /// Hashing one would silently pull it down — a multi-gigabyte download nobody asked for.
    /// </summary>
    private const FileAttributes Placeholder =
        FileAttributes.Offline | PreviewClassifier.RecallOnOpen | PreviewClassifier.RecallOnDataAccess;

    public FileFingerprint? Hash(string path, long maxBytes, Action<long>? progress, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        try
        {
            var info = new FileInfo(path);
            if (!info.Exists) return null;

            // A reparse point is the one entry it is, not the file it points at — the same reading
            // DeleteSurveyor takes. Following it would hash the target a second time under a name
            // that is not really a copy of anything.
            if ((info.Attributes & FileAttributes.ReparsePoint) != 0) return null;
            if ((info.Attributes & Placeholder) != 0) return null;
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return null;
        }

        FileStream stream;
        try
        {
            stream = new FileStream(path, new FileStreamOptions
            {
                Mode = FileMode.Open,
                Access = FileAccess.Read,
                Share = FileShare.ReadWrite | FileShare.Delete,
                Options = FileOptions.SequentialScan,

                // The loop below reads a megabyte at a time; a second layer of buffering underneath
                // it would only copy every byte an extra time.
                BufferSize = 0,
            });
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return null;
        }

        using (stream)
        {
            FileIdentity? identity = null;
            if (FileIdentityNative.TryRead(stream.SafeFileHandle, out var links, out var id) && links > 1)
                identity = new FileIdentity(id.Volume, id.Index);

            using var digest = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
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
                    catch (Exception ex) when (IsReadFailure(ex))
                    {
                        // Failing part-way is still a failure: a hash of the first half of a file
                        // would compare equal to nothing and unequal to everything, silently.
                        return null;
                    }

                    if (got == 0) break;

                    digest.AppendData(buffer, 0, got);
                    read += got;
                    progress?.Invoke(got);
                }

                return new FileFingerprint(Convert.ToHexString(digest.GetHashAndReset()), read, identity);
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
            }
        }
    }

    /// <summary>The same failure set the surveyor and the transfer executor use.</summary>
    private static bool IsReadFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
