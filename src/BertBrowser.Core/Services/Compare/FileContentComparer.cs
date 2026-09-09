using System.Buffers;
using BertBrowser.Core.Interop;

namespace BertBrowser.Core.Services.Compare;

public enum ContentVerdict
{
    /// <summary>Byte for byte the same.</summary>
    Identical,

    /// <summary>Not the same. <see cref="ContentComparison.FirstDifferenceOffset"/> says where.</summary>
    Differs,

    /// <summary>
    /// A side could not be read. Never <see cref="Identical"/> by omission — this is the verdict
    /// that must not be mistaken for a match, because a match is what authorises a delete.
    /// </summary>
    Unreadable,
}

/// <param name="FirstDifferenceOffset">
/// Where the two first disagree, or — when one is a prefix of the other — where the shorter ends.
/// Null only when they are identical.
/// </param>
public sealed record ContentComparison(
    ContentVerdict Verdict,
    long? FirstDifferenceOffset,
    long LeftBytes,
    long RightBytes)
{
    public bool Identical => Verdict is ContentVerdict.Identical;
}

/// <summary>Comparing two files by their bytes.</summary>
public interface IFileContentComparer
{
    ContentComparison Compare(string leftPath, string rightPath, Action<long>? progress, CancellationToken ct);
}

/// <summary>
/// The real thing, over <see cref="ReadOnlyFile"/>.
/// </summary>
/// <remarks>
/// <para>
/// This namespace already owns "are these the same", and the folder comparison is the caller that
/// matters most: a timestamp says two files are probably different, and only this can say they are
/// not. So the rule it has to keep is the one <see cref="CompareVerdict.Same"/> states — every
/// doubt resolves away from a match. A read that fails is <see cref="ContentVerdict.Unreadable"/>,
/// never a verdict.
/// </para>
/// <para>
/// Opening goes through <see cref="ReadOnlyFile"/>, which is why that was extracted: this is the
/// third caller of the share-flag and placeholder rules, and a third copy of them is a third chance
/// for one to drift.
/// </para>
/// </remarks>
public sealed class FileContentComparer : IFileContentComparer
{
    private const int BufferBytes = 1024 * 1024;

    public ContentComparison Compare(string leftPath, string rightPath, Action<long>? progress, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        if (ReadOnlyFile.TryOpen(leftPath) is not { } left)
            return new ContentComparison(ContentVerdict.Unreadable, null, 0, 0);

        using (left)
        {
            if (ReadOnlyFile.TryOpen(rightPath) is not { } right)
                return new ContentComparison(ContentVerdict.Unreadable, null, left.Length, 0);

            using (right)
            {
                // The same file under two names — a hard link, or the same path twice. A handle
                // already knows this, and confirming it by reading two identical gigabytes would be
                // minutes spent learning what Windows was willing to say immediately.
                if (SameFile(left, right))
                    return new ContentComparison(ContentVerdict.Identical, null, left.Length, right.Length);

                return CompareBytes(left, right, progress, ct);
            }
        }
    }

    private static bool SameFile(FileStream left, FileStream right) =>
        FileIdentityNative.TryRead(left.SafeFileHandle, out _, out var a)
        && FileIdentityNative.TryRead(right.SafeFileHandle, out _, out var b)
        && a.Volume == b.Volume && a.Index == b.Index;

    private static ContentComparison CompareBytes(
        FileStream left, FileStream right, Action<long>? progress, CancellationToken ct)
    {
        long leftLength = left.Length, rightLength = right.Length;

        var a = ArrayPool<byte>.Shared.Rent(BufferBytes);
        var b = ArrayPool<byte>.Shared.Rent(BufferBytes);

        try
        {
            var offset = 0L;

            while (true)
            {
                ct.ThrowIfCancellationRequested();

                int gotA, gotB;
                try
                {
                    gotA = left.ReadAtLeast(a, a.Length, throwOnEndOfStream: false);
                    gotB = right.ReadAtLeast(b, b.Length, throwOnEndOfStream: false);
                }
                catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
                {
                    // Failing part-way through cannot produce a verdict either way: the bytes read
                    // so far being equal is not evidence that the rest are.
                    return new ContentComparison(ContentVerdict.Unreadable, null, leftLength, rightLength);
                }

                var common = Math.Min(gotA, gotB);
                var spanA = a.AsSpan(0, common);
                var spanB = b.AsSpan(0, common);

                if (!spanA.SequenceEqual(spanB))
                {
                    return new ContentComparison(
                        ContentVerdict.Differs, offset + FirstDifference(spanA, spanB), leftLength, rightLength);
                }

                // One ran out before the other: a prefix. The offset is where the shorter ended,
                // which is a more useful thing to say than "the sizes differ".
                if (gotA != gotB)
                    return new ContentComparison(ContentVerdict.Differs, offset + common, leftLength, rightLength);

                if (gotA == 0) break;

                offset += gotA;
                progress?.Invoke(gotA);
            }

            return new ContentComparison(ContentVerdict.Identical, null, leftLength, rightLength);
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(a);
            ArrayPool<byte>.Shared.Return(b);
        }
    }

    /// <summary>
    /// The exact byte, once a chunk is known to differ.
    /// </summary>
    /// <remarks>
    /// <c>CommonPrefixLength</c> is vectorised, so narrowing a megabyte to one byte costs nothing
    /// measurable and the answer is an offset a person can go and look at.
    /// </remarks>
    private static int FirstDifference(ReadOnlySpan<byte> a, ReadOnlySpan<byte> b) =>
        a.CommonPrefixLength(b);
}
