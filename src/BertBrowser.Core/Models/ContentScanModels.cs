namespace BertBrowser.Core.Models;

/// <summary>
/// Where a content search found its needle in one file: the line, and where in that line.
/// </summary>
/// <remarks>
/// This is what makes a grep result actionable rather than a list of filenames — you can tell a
/// real hit from a coincidence without opening anything. It rides on <see cref="SearchHit"/> rather
/// than on the streamed batch, because the list is rebuilt from the final hits when the search
/// completes: a match attached only to the live batch would blank the column at the finish.
/// </remarks>
/// <param name="LineNumber">1-based, counted in the file as read.</param>
/// <param name="Line">The matching line, clipped around the match if it was very long.</param>
/// <param name="MatchStart">Where the needle starts <em>within <paramref name="Line"/></em>.</param>
/// <param name="MatchLength">How long the needle is.</param>
public sealed record ContentMatch(int LineNumber, string Line, int MatchStart, int MatchLength);

/// <summary>Live progress of the reading pass, for the status bar.</summary>
/// <param name="FilesRead">Files opened so far.</param>
/// <param name="FilesToRead">How many the pass expects to open.</param>
/// <param name="BytesRead">Bytes read so far.</param>
public readonly record struct ContentScanProgress(int FilesRead, int FilesToRead, long BytesRead);

/// <summary>Which ceiling a content search ran into, if any.</summary>
public enum ContentScanLimit
{
    /// <summary>It finished: every candidate was examined.</summary>
    None,

    /// <summary>The first pass filled up, so files past it were never considered.</summary>
    Candidates,

    /// <summary>The file-open ceiling was reached.</summary>
    Files,

    /// <summary>The byte ceiling was reached.</summary>
    Bytes,
}

/// <summary>
/// What the reading pass actually did — the honest account behind an empty or short result.
/// </summary>
/// <remarks>
/// <para>Modelled on <c>DuplicateScanOutcome</c>, which keeps "the user stopped it" and "something
/// could not be read" as separate flags for a stated reason: conflating them makes a cancelled run
/// look like a disk full of broken files.</para>
/// <para><see cref="LastPathExamined"/> is the field worth defending. A whole-PC content search
/// scans the index in path order with no <c>ORDER BY</c>, so hitting the candidate ceiling means
/// stopping somewhere alphabetical — very possibly before <c>C:\Users</c>. "Searched the first
/// 50,000 files" is true and still reads as "your PC has no such file"; naming where it got to is
/// the difference between a bound and a lie, the same instinct behind
/// <c>DiskUsageRules.Unaccounted</c> returning null rather than a smaller number.</para>
/// </remarks>
/// <param name="FilesRead">How many files were opened and searched.</param>
/// <param name="BytesRead">How many bytes of them were read.</param>
/// <param name="Limit">Which ceiling stopped it, if any.</param>
/// <param name="Unreadable">How many candidates could not be read at all.</param>
/// <param name="Truncated">How many were longer than the per-file budget, so only their front was searched.</param>
/// <param name="LastPathExamined">The last candidate considered, for saying where a ceiling landed.</param>
public sealed record ContentScanReport(
    int FilesRead,
    long BytesRead,
    ContentScanLimit Limit = ContentScanLimit.None,
    int Unreadable = 0,
    int Truncated = 0,
    string? LastPathExamined = null)
{
    /// <summary>
    /// Whether the answer is a floor rather than the answer — something was skipped or cut short.
    /// </summary>
    public bool Incomplete =>
        Limit != ContentScanLimit.None || Unreadable > 0 || Truncated > 0;
}
