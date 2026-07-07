namespace BertBrowser.Core.Models;

/// <summary>A row destined for (or read from) the fs_entry search index.
/// <paramref name="Hidden"/> is the entry's effective hidden state — its own Hidden
/// attribute or that of any ancestor within the crawled subtree.</summary>
public sealed record FsEntryRow(
    string PathKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden = false);
