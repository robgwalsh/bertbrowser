namespace BertBrowser.Core.Models;

/// <summary>A row destined for (or read from) the fs_entry search index.</summary>
public sealed record FsEntryRow(
    string PathKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc);
