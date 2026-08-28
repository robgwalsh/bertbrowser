using System.IO.Compression;

namespace BertBrowser.Core.Services.Preview;

/// <summary>One entry inside an archive. Nothing is extracted to produce it.</summary>
public sealed record ArchiveEntry(
    string Path,
    long SizeBytes,
    long CompressedBytes,
    DateTime Modified,
    bool IsDirectory);

/// <summary>What an archive holds, or why it could not be read.</summary>
/// <param name="Entries">Up to the entry cap, ordered by path.</param>
/// <param name="TotalCount">Every entry, including the ones past the cap.</param>
/// <param name="Truncated">There are more entries than <paramref name="Entries"/> shows.</param>
/// <param name="Error">Set when the archive could not be opened; <paramref name="Entries"/> is
/// then empty. A damaged archive is a message, never a throw — a preview must not be able to fail
/// louder than the thing it is previewing.</param>
public sealed record ArchiveContents(
    IReadOnlyList<ArchiveEntry> Entries,
    int TotalCount,
    long TotalBytes,
    long TotalCompressedBytes,
    bool Truncated,
    string? Error)
{
    public static ArchiveContents Failed(string error) => new([], 0, 0, 0, false, error);

    /// <summary>Ratio of packed to unpacked, 0 when there is nothing to compare.</summary>
    public double CompressionRatio =>
        TotalBytes > 0 ? 1.0 - (double)TotalCompressedBytes / TotalBytes : 0;
}

/// <summary>
/// Lists a zip container's contents from its central directory. Only the directory is read — no
/// entry is ever opened, decompressed or written anywhere, so previewing an archive costs the same
/// whether it holds one file or a gigabyte of them.
/// </summary>
public static class ArchiveListing
{
    /// <summary>Rows past this are counted but not listed. A `node_modules` zip has hundreds of
    /// thousands of entries and nobody scrolls them in a preview pane.</summary>
    public const int DefaultMaxEntries = 1_000;

    public static ArchiveContents Read(Stream stream, int maxEntries = DefaultMaxEntries)
    {
        try
        {
            return ReadCore(stream, Math.Max(0, maxEntries));
        }
        catch (InvalidDataException)
        {
            return ArchiveContents.Failed("Not a readable archive, or damaged.");
        }
        catch (Exception ex) when (ex is IOException or NotSupportedException or ObjectDisposedException)
        {
            return ArchiveContents.Failed("The archive could not be read.");
        }
    }

    private static ArchiveContents ReadCore(Stream stream, int maxEntries)
    {
        using var archive = new ZipArchive(stream, ZipArchiveMode.Read, leaveOpen: true);

        var rows = new List<ArchiveEntry>(Math.Min(maxEntries, archive.Entries.Count));
        long totalBytes = 0, totalCompressed = 0;

        foreach (var entry in archive.Entries)
        {
            totalBytes += entry.Length;
            totalCompressed += entry.CompressedLength;

            // A folder inside a zip is a zero-length entry whose name ends in a separator; it
            // carries no data and is listed for shape, not for size.
            var isDirectory = entry.FullName.EndsWith('/') || entry.FullName.EndsWith('\\');

            rows.Add(new ArchiveEntry(
                entry.FullName.Replace('/', '\\'),
                isDirectory ? 0 : entry.Length,
                isDirectory ? 0 : entry.CompressedLength,
                entry.LastWriteTime.LocalDateTime,
                isDirectory));
        }

        var total = rows.Count;
        rows.Sort(static (a, b) => string.Compare(a.Path, b.Path, StringComparison.OrdinalIgnoreCase));

        var truncated = total > maxEntries;
        if (truncated) rows.RemoveRange(maxEntries, total - maxEntries);

        return new ArchiveContents(rows, total, totalBytes, totalCompressed, truncated, Error: null);
    }
}
