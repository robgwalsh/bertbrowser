using System.Buffers;
using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Core.Services.Search;

/// <summary>Reads one file's text for a content search.</summary>
/// <remarks>
/// <para>An interface for the reason <c>IFileHasher</c> is one: it is what lets a cancel land in
/// the <em>middle</em> of a file deterministically, in a test that takes milliseconds instead of
/// writing a multi-gigabyte fixture and racing it.</para>
/// <para><strong>Null and a throw mean different things, and the difference is load-bearing.</strong>
/// Null is this file having a problem — the scan carries on and reports itself incomplete, exactly
/// as one item's failure never costs the others anywhere else in this app. An
/// <see cref="OperationCanceledException"/> is the whole run stopping. Conflate them and a cancel
/// looks like a disk full of unreadable files.</para>
/// </remarks>
public interface IContentReader
{
    /// <returns>
    /// The file's text; <see cref="ContentText.None"/> when there is nothing to search (it is not
    /// text); or null when it could not or must not be read.
    /// </returns>
    ContentText? Read(string path, long maxBytes, CancellationToken ct);
}

/// <summary>The real one, over the filesystem.</summary>
public sealed class FileSystemContentReader : IContentReader
{
    /// <summary>
    /// A cloud file whose bytes are not on this machine, per the two attributes .NET does not name.
    /// </summary>
    /// <remarks>
    /// Reading one makes the provider fetch it. A search that quietly pulled a OneDrive folder down
    /// from the cloud would be a multi-gigabyte download nobody asked for, so these are refused and
    /// counted — the result is then honestly a floor. <c>PreviewClassifier</c> and
    /// <c>FileSystemFileHasher</c> refuse the same set for the same reason.
    /// </remarks>
    private const FileAttributes Placeholder =
        FileAttributes.Offline | PreviewClassifier.RecallOnOpen | PreviewClassifier.RecallOnDataAccess;

    public ContentText? Read(string path, long maxBytes, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        try
        {
            var info = new FileInfo(path);
            if (!info.Exists) return null;

            // A reparse point is the one entry it is, not the file it points at — the reading
            // DeleteSurveyor and the hasher both take. Following it would search a file that is
            // somewhere else, under a name that only looks like it lives here.
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

                // The rule, not a detail: a held handle is what would block this app's own rename,
                // move and delete executors — in the very folder the user is searching.
                Share = FileShare.ReadWrite | FileShare.Delete,
                Options = FileOptions.SequentialScan,

                // The reads below are already large; a second layer of buffering underneath would
                // only copy every byte an extra time.
                BufferSize = 0,
            });
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return null;
        }

        using (stream)
        {
            var budget = (int)Math.Clamp(maxBytes, 0, int.MaxValue - 1);

            // One byte past the budget, so "is there more?" is answered without seeking or
            // trusting a length that may be stale — the trick TextPreviewReader.ReadAtMost uses,
            // over a pooled buffer because this runs tens of thousands of times.
            var buffer = ArrayPool<byte>.Shared.Rent(budget + 1);
            try
            {
                var head = Fill(stream, buffer, 0, Math.Min(ContentSearchRules.HeadSampleBytes, budget + 1), ct);
                if (head < 0) return null;

                var atEnd = head < Math.Min(ContentSearchRules.HeadSampleBytes, budget + 1);

                // Decide from the head whether this is worth reading whole, so a binary costs
                // 8 KB rather than a megabyte. IsConvincingText is the stricter of the pair
                // because here we really are guessing: nothing said this file was text.
                var sample = TextPreviewReader.Decode(
                    buffer.AsSpan(0, Math.Min(head, budget)), moreRemains: !atEnd,
                    maxLines: int.MaxValue, maxLineLength: 0);
                if (!TextPreviewReader.IsConvincingText(sample)) return ContentText.None;

                if (atEnd)
                    return new ContentText(sample.Text, sample.Truncated);

                var total = Fill(stream, buffer, head, budget + 1, ct);
                if (total < 0) return null;

                var moreRemains = total > budget;
                var body = TextPreviewReader.Decode(
                    buffer.AsSpan(0, Math.Min(total, budget)), moreRemains,
                    maxLines: int.MaxValue, maxLineLength: 0);

                // Both caps are deliberately lifted. The preview pane stops at 5,000 lines and
                // folds anything past 4,096 characters, which are the right rules for a text
                // control and the wrong ones here: the first would stop searching at line 5,000,
                // and the second would insert line breaks that the snippet's line numbers then
                // counted. Clipping a long line is the snippet's job, not the reader's.
                return body.LooksBinary ? ContentText.None : new ContentText(body.Text, moreRemains);
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
            }
        }
    }

    /// <summary>Reads until <paramref name="upTo"/> or end of file. -1 when the read failed.</summary>
    private static int Fill(Stream stream, byte[] buffer, int from, int upTo, CancellationToken ct)
    {
        var total = from;
        while (total < upTo)
        {
            ct.ThrowIfCancellationRequested();

            int got;
            try
            {
                got = stream.Read(buffer, total, upTo - total);
            }
            catch (Exception ex) when (IsReadFailure(ex))
            {
                // Failing part-way is still a failure. Searching the first half of a file and
                // reporting "not found" would be a wrong answer rather than a missing one.
                return -1;
            }

            if (got == 0) break;
            total += got;
        }
        return total;
    }

    /// <summary>The same failure set the surveyor, the hasher and the transfer executor use.</summary>
    private static bool IsReadFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
