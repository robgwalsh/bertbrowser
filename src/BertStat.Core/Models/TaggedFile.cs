namespace BertStat.Core.Models;

/// <summary>One row of the recursive tag-filter query.</summary>
public sealed record TaggedFile(long FileId, string DisplayPath, IReadOnlyList<string> Tags);
