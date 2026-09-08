namespace BertBrowser.Core.Models;

/// <summary>
/// One search result. <paramref name="RelativeDirDisplay"/> is the hit's parent
/// directory relative to the search root ("" for direct children), display-cased.
/// </summary>
/// <param name="Match">
/// Where a <c>content:</c> term found its needle, when one did. Null for every other search, and
/// for a content hit the <em>name</em> settled without the file being opened — there is no line to
/// point at in that case, and inventing one would be a lie about where the match was.
/// </param>
/// <param name="Attributes">
/// The entry's own attribute mask, so the Attributes column reads the same on a search row as on a
/// directory listing. Zero means "not recorded" rather than "none" — an archive entry has no
/// Windows attributes, and the largest-files and duplicate scans do not select the column.
/// </param>
/// <param name="CreatedUtc"><see cref="DateTime.MinValue"/> when unknown, and rendered blank.</param>
public sealed record SearchHit(
    string DisplayPath,
    string RelativeDirDisplay,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden = false,
    FileAttributes Attributes = 0,
    DateTime CreatedUtc = default,
    ContentMatch? Match = null);
