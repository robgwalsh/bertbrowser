namespace BertBrowser.Core.Models;

/// <summary>A row destined for (or read from) the fs_entry search index.</summary>
/// <param name="Hidden">The entry's <em>effective</em> hidden state — its own Hidden attribute or
/// that of any ancestor within the crawled subtree.</param>
/// <param name="Attributes">The entry's <em>own</em> Win32 attribute mask, uninherited — unlike
/// <paramref name="Hidden"/>, which is OR'd down the tree so a query can filter on one column.
/// Zero means "not recorded": rows written before the attributes column existed carry it, and so
/// does an archive entry, which has no Windows attributes at all.</param>
/// <param name="CreatedUtc"><see cref="DateTime.MinValue"/> when unknown — the names-only USN
/// build path leaves it so, exactly as it does <paramref name="ModifiedUtc"/>.</param>
public sealed record FsEntryRow(
    string PathKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden = false,
    FileAttributes Attributes = 0,
    DateTime CreatedUtc = default);
