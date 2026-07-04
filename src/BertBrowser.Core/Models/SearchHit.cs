namespace BertBrowser.Core.Models;

/// <summary>
/// One search result. <paramref name="RelativeDirDisplay"/> is the hit's parent
/// directory relative to the search root ("" for direct children), display-cased.
/// </summary>
public sealed record SearchHit(
    string DisplayPath,
    string RelativeDirDisplay,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc);
