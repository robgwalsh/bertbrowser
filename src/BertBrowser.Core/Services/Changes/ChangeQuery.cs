namespace BertBrowser.Core.Services.Changes;

/// <summary>What the timeline asks the log for.</summary>
/// <param name="SinceUtc">The lower bound; there is no upper one, the log ends now.</param>
/// <param name="ScopePathKey">A canonical directory key to stay under, or null for every volume.</param>
/// <param name="Kinds">Which kinds to include. All four means no filter.</param>
/// <param name="Limit">The most rows wanted; one more is read to learn whether there were more.</param>
public sealed record ChangeQuery(
    DateTime SinceUtc,
    string? ScopePathKey,
    IReadOnlySet<ChangeKind> Kinds,
    bool IncludeHidden,
    int Limit);

/// <summary>One row of <c>fs_change</c>, read back.</summary>
public sealed record ChangeRow(
    long Id,
    string PathKey,
    string DisplayPath,
    string? OldDisplayPath,
    bool IsDirectory,
    bool Hidden,
    ChangeKind Kind,
    DateTime FirstUtc,
    DateTime LastUtc,
    int Count);

/// <summary>
/// The cheapest thing that changes whenever the log does. The largest id alone is not enough: a
/// coalesced write bumps an existing row and adds none, which is most of what an installer does
/// after its first minute.
/// </summary>
public readonly record struct ChangeLogStamp(long MaxId, string? LastUtc);
