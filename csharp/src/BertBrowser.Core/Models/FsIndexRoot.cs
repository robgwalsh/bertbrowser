namespace BertBrowser.Core.Models;

/// <summary>An indexed subtree root (fs_index_root row).</summary>
public sealed record FsIndexRoot(
    string PathKey,
    string DisplayPath,
    DateTime CrawledUtc,
    bool Complete,
    bool Stale);
