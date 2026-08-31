namespace BertBrowser.Core.Services.Search;

/// <summary>
/// What a content search is allowed to spend, and why each number is the number it is.
/// </summary>
/// <remarks>
/// <para>These are ceilings, not targets. Nothing is refused for being too broad — a query that
/// reaches one of them still returns everything it found, and says which ceiling it hit. The
/// alternative, refusing an unnarrowed <c>content:</c>, is worse: the narrowing is often the folder
/// you are standing in, and there is no way for a parser to see that.</para>
/// <para><strong>Measured rather than guessed</strong>, warm, against a real 245,305-file tree:
/// an 8 KB head sniff runs at 15,609 files/s on four threads (8,436 on one, 13,158 on eight — four
/// is the peak), and reading text files whole runs at ~13,100 files/s and ~49 MB/s. Matching itself
/// is free: a substring miss over 50 M characters takes 7 ms, and ignoring case costs the same as
/// not ignoring it, because both are vectorised. So every ceiling here is denominated in file opens
/// and bytes, never in terms — a second <c>content:</c> term costs nothing.</para>
/// </remarks>
public static class ContentSearchRules
{
    /// <summary>How much of a file is read before deciding whether it is text at all.</summary>
    /// <remarks>
    /// The same two-stage shape <c>DuplicateScanner</c> uses, for the same reason: a binary then
    /// costs 8 KB instead of a megabyte. It is also what the encoding is decided from — a
    /// deliberate difference from the preview pane, which sees the whole buffer before choosing
    /// between UTF-8 and Latin-1. Deciding from the head is the right trade when the alternative is
    /// reading every candidate twice, but it is a difference, so it is written down here rather
    /// than left for someone to "fix" later.
    /// </remarks>
    public const int HeadSampleBytes = 8 * 1024;

    /// <summary>How much of any one file is searched.</summary>
    /// <remarks>
    /// <c>PreviewClassifier.DefaultTextBudget</c>'s number, deliberately: it is the same judgement
    /// about the same files, and one place to argue about it is better than two. A miss against a
    /// file longer than this is "not in the first megabyte", which the outcome reports rather than
    /// passing off as "not in the file".
    /// </remarks>
    public const long MaxBytesPerFile = 1L << 20;

    /// <summary>How many first-pass candidates a content query asks the index or the walk for.</summary>
    /// <remarks>
    /// The ordinary result cap is 1,000, and using it here would mean never grepping more than a
    /// thousand files — most of which are about to be discarded, since the point of the pass is
    /// that the name did not settle them. At the measured sniff rate 50,000 candidates is a little
    /// over three seconds of worst case, and it costs the query nothing extra to fetch: the rows
    /// come off a scan that was happening anyway.
    /// </remarks>
    public const int MaxCandidates = 50_000;

    /// <summary>How many files one search will open.</summary>
    /// <remarks>
    /// A separate number from <see cref="MaxCandidates"/> because they are separate populations:
    /// a candidate the name already settled is never opened, so <c>content:a OR ext:md</c> can
    /// examine far more rows than it reads files.
    /// </remarks>
    public const int MaxFilesOpened = 50_000;

    /// <summary>And how many bytes of them, whichever ceiling is reached first.</summary>
    /// <remarks>
    /// About ten seconds at the measured 49 MB/s. A typical 50,000-file shortlist averages well
    /// under this; it is here for the folder of large logs, where the file ceiling alone would
    /// allow tens of gigabytes.
    /// </remarks>
    public const long MaxBytesRead = 512L * 1024 * 1024;

    /// <summary>How many files are read at once.</summary>
    /// <remarks>
    /// Four, measured: 15,609 files/s against 8,436 at one thread and 13,158 at eight. It matches
    /// <c>DuplicateScanner.MaxParallelism</c> and its stated reason — past this, a spinning disk
    /// turns a sequential pass into seek thrashing. Treat it as a ceiling rather than a floor: on
    /// an unindexed subtree this runs beside <c>IndexCrawler</c> over the same tree, so raising it
    /// means re-measuring against that, not against an idle disk.
    /// </remarks>
    public const int MaxParallelism = 4;
}
