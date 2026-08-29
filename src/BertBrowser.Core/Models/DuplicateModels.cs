namespace BertBrowser.Core.Models;

/// <summary>
/// What standing a duplicate scan's answer has.
/// </summary>
/// <remarks>
/// <para>
/// This is deliberately <em>not</em> <see cref="DiskUsageAvailability"/>, for the same reason
/// <c>DiskUsageRules.ClassifyBreakdown</c> is deliberately not <c>Classify</c>: it weighs different
/// evidence. That rule reads an all-zero result as a sizeless volume, which it can do because it
/// orders by size and so knows the largest row in range. A duplicate scan applies a size
/// <em>floor</em> before it looks at anything, so an empty result tells it nothing at all — it has
/// to ask the index directly whether it holds believable sizes.
/// </para>
/// <para>
/// The state that matters is <see cref="NoSizeData"/>. The
/// <c>FSCTL_ENUM_USN_DATA</c> fallback build writes every row with <c>size_bytes = 0</c>, and
/// grouping by size on such a volume would put the entire disk in one candidate group. Reporting
/// "no duplicates" there would be a lie, and reporting the group would be worse.
/// </para>
/// </remarks>
public enum DuplicateScanAvailability
{
    /// <summary>The index holds real sizes for this scope; the answer can be believed.</summary>
    Ready,

    /// <summary>A volume's initial enumeration is still running, so the answer is a floor.</summary>
    Building,

    /// <summary>Nothing has indexed this scope. There is no shortlist to draw from.</summary>
    NotIndexed,

    /// <summary>Indexed by name only — every size is zero, so sizes cannot shortlist anything.</summary>
    NoSizeData,
}

/// <summary>Which pass of the scan is running.</summary>
public enum DuplicateScanPhase
{
    /// <summary>Reading the index for files that share a byte length. Touches no file.</summary>
    Shortlisting,

    /// <summary>Hashing the first <c>HeadSampleBytes</c> of each candidate.</summary>
    Sampling,

    /// <summary>Hashing the survivors in full.</summary>
    Hashing,
}

/// <summary>What to scan.</summary>
/// <param name="RootPath">The folder to look under, or null for every indexed volume.</param>
/// <param name="MinSizeBytes">
/// The floor below which files are not considered. Not cosmetic: it is what bounds the shortlist's
/// memory and the hashing passes' read volume, and duplicate 400-byte files are not what anyone
/// opens this for.
/// </param>
/// <param name="SkipSystemFolders">
/// Leave out Windows, Program Files and the rest. On by default: duplicates there are not the
/// user's to remove, and <c>WinSxS</c> alone would dominate every result.
/// </param>
public sealed record DuplicateScanRequest(
    string? RootPath = null,
    long MinSizeBytes = DuplicateScanRequest.DefaultMinSizeBytes,
    bool IncludeHidden = false,
    bool SkipSystemFolders = true)
{
    /// <summary>One megabyte.</summary>
    public const long DefaultMinSizeBytes = 1024 * 1024;
}

/// <summary>
/// One file in a duplicate group.
/// </summary>
/// <param name="HardlinkPaths">
/// The other names this same file on disk answers to, when it has more than one. Empty for the
/// overwhelming majority. These are <em>not</em> duplicates — deleting one frees nothing — so they
/// travel with the entry rather than appearing beside it.
/// </param>
public sealed record DuplicateFile(
    string DisplayPath,
    string RelativeDirDisplay,
    string Name,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden,
    IReadOnlyList<string> HardlinkPaths)
{
    public DuplicateFile(
        string displayPath, string relativeDirDisplay, string name,
        long sizeBytes, DateTime modifiedUtc, bool hidden)
        : this(displayPath, relativeDirDisplay, name, sizeBytes, modifiedUtc, hidden, [])
    {
    }
}

/// <summary>Files that are byte-for-byte the same thing, in different places.</summary>
public sealed record DuplicateGroup(long SizeBytes, string Hash, IReadOnlyList<DuplicateFile> Files)
{
    /// <summary>What keeping every copy costs: one copy is wanted, the rest are the waste.</summary>
    public long WastedBytes => SizeBytes * (Files.Count - 1);
}

/// <summary>
/// Progress while a scan runs.
/// </summary>
/// <remarks>
/// <see cref="BytesTotal"/> is zero during <see cref="DuplicateScanPhase.Shortlisting"/> and the
/// bar must be indeterminate there: counting the rows first would cost a second full scan of the
/// index, and a determinate bar pinned at 0% reads as a stall. It becomes exact the moment the
/// shortlist is in, which is what the two hashing phases show.
/// </remarks>
public sealed record DuplicateScanProgress(
    DuplicateScanPhase Phase,
    int Done,
    int Total,
    string CurrentName,
    long BytesDone = 0,
    long BytesTotal = 0);

/// <summary>What a scan found.</summary>
/// <param name="Cancelled">
/// True when the user stopped it. Without this a cancelled scan is indistinguishable from one that
/// genuinely found nothing.
/// </param>
/// <param name="Incomplete">
/// True when at least one candidate could not be read. The groups are then a floor, not the answer,
/// which the view says rather than quietly under-reporting.
/// </param>
public sealed record DuplicateScanOutcome(
    IReadOnlyList<DuplicateGroup> Groups,
    DuplicateScanAvailability Availability,
    bool Cancelled = false,
    bool Incomplete = false,
    int FilesHashed = 0,
    long BytesHashed = 0)
{
    public long WastedBytes => Groups.Sum(g => g.WastedBytes);

    public bool HasResults => Groups.Count > 0;

    public static DuplicateScanOutcome Empty(DuplicateScanAvailability availability) =>
        new([], availability);
}

/// <summary>
/// What the index had to say when asked for a duplicate shortlist.
/// </summary>
/// <param name="Files">
/// The files sharing a byte length with at least one other, at or above the requested floor.
/// </param>
/// <param name="FilesInScope">
/// Every file row in scope whatever its length, counted before any exclusion — this describes what
/// the index knows, not what the caller asked to see.
/// </param>
/// <param name="SizedFilesInScope">
/// How many of those carry a real byte length. Zero against a non-zero <paramref name="FilesInScope"/>
/// is the signature of the sizeless build path, and the whole reason both numbers are carried.
/// </param>
public sealed record DuplicateShortlist(
    IReadOnlyList<SearchHit> Files, int FilesInScope, int SizedFilesInScope);
