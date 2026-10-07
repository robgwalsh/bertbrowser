namespace BertBrowser.Core.Services.Timestamps;

/// <summary>Where the new date comes from.</summary>
public enum TimestampSource
{
    /// <summary>One date and time, the same for every item.</summary>
    Value,
    /// <summary>Each item's own date, moved by an offset.</summary>
    Shift,
    /// <summary>Each picture's own date taken.</summary>
    DateTaken,
}

/// <summary>
/// What to do to a selection's dates.
/// </summary>
/// <param name="Value">Local time, for <see cref="TimestampSource.Value"/>.</param>
public sealed record TimestampChange(
    bool Modified,
    bool Created,
    TimestampSource Source,
    DateTime? Value = null,
    TimeSpan? Offset = null);

/// <summary>What the planner is told about one entry on disk.</summary>
public sealed record TimestampEntry(FileAttributes Attributes, DateTime ModifiedUtc, DateTime CreatedUtc);

public enum TimestampRejection
{
    Missing,
    /// <summary>An entry inside an archive has no dates of its own on disk.</summary>
    InsideArchive,
    /// <summary>Setting a link's date would set its target's.</summary>
    Link,
    NoDateTaken,
    /// <summary>The result is not a date Windows can store.</summary>
    OutOfRange,
    /// <summary>It already has exactly these dates.</summary>
    Unchanged,
}

public sealed record RejectedTimestamp(string Path, TimestampRejection Reason, string Message);

/// <summary>One item and the dates it should end up with. A null is left alone.</summary>
public sealed record PlannedTimestamp(string Path, bool IsDirectory, DateTime? ModifiedUtc, DateTime? CreatedUtc);

public sealed record TimestampPlan(
    IReadOnlyList<PlannedTimestamp> Items,
    IReadOnlyList<RejectedTimestamp> Rejected)
{
    public bool HasWork => Items.Count > 0;

    public static TimestampPlan Empty { get; } = new([], []);
}

/// <summary>
/// An item whose dates were changed: what they were and what they are, which is everything an
/// undo needs — there is no data to hold, only four numbers.
/// </summary>
public sealed record StampedEntry(
    string Path,
    bool IsDirectory,
    DateTime BeforeModifiedUtc,
    DateTime BeforeCreatedUtc,
    DateTime AfterModifiedUtc,
    DateTime AfterCreatedUtc);

public sealed record FailedTimestamp(string Path, string Message, bool AccessDenied = false);

public sealed record TimestampOutcome(
    IReadOnlyList<StampedEntry> Stamped,
    IReadOnlyList<FailedTimestamp> Failed)
{
    public bool CanUndo => Stamped.Count > 0;

    public static TimestampOutcome Empty { get; } = new([], []);
}
