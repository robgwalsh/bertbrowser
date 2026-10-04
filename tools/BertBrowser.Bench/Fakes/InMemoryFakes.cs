using System.Globalization;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Duplicates;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Bench.Fakes;

/// <summary>A duplicate shortlist that is simply handed over, so the scanner is measured alone.</summary>
internal sealed class ListCandidateSource(DuplicateShortlist shortlist) : IDuplicateCandidateSource
{
    public DuplicateShortlist Shortlist(DuplicateScanRequest request, CancellationToken ct) => shortlist;
}

/// <summary>
/// A hasher that answers from a table instead of a disk: the hash is the file's "content id", so
/// two paths with the same id are duplicates and the scanner's grouping is exercised without a read.
/// </summary>
internal sealed class TableHasher(IReadOnlyDictionary<string, (int ContentId, long Size)> table) : IFileHasher
{
    public FileFingerprint? Hash(string path, long maxBytes, Action<long>? progress, CancellationToken ct)
    {
        if (!table.TryGetValue(path, out var entry)) return null;
        var read = Math.Min(maxBytes, entry.Size);
        progress?.Invoke(read);
        return new FileFingerprint(entry.ContentId.ToString("X16", CultureInfo.InvariantCulture), read, null);
    }
}

/// <summary>File text from a table, so a content scan is the scanner plus the decode ladder and
/// nothing of the file system.</summary>
internal sealed class InMemoryContentReader(IReadOnlyDictionary<string, ContentText> table) : IContentReader
{
    public ContentText? Read(string path, long maxBytes, CancellationToken ct) =>
        table.TryGetValue(path, out var text) ? text : null;
}

/// <summary>Watches nothing: the search service's "is this root kept fresh" question, answered no.</summary>
internal sealed class NoWatchers : IIndexWatcherService
{
    public bool IsWatching(string rootKey) => false;

    public void Watch(string rootKey, string displayPath)
    {
    }

    public void Dispose()
    {
    }
}
