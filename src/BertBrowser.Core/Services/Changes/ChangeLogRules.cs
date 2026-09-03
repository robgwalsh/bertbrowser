using BertBrowser.Core.Interop;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Changes;

/// <summary>
/// The decisions behind the change timeline, kept pure so they are tested without a volume, a
/// database or a window.
/// </summary>
public static class ChangeLogRules
{
    /// <summary>Repeated writes to one file inside this window fold into one row with a count.</summary>
    public static readonly TimeSpan CoalesceWindow = TimeSpan.FromSeconds(60);

    /// <summary>The hard cap, whatever the retention: a runaway build must not fill the disk.</summary>
    public const int MaxRows = 500_000;

    /// <summary>How often the writer prunes. Wall-clock, not per-batch: the tail flushes on nearly
    /// every one-second poll, so a batch count would be a worse clock.</summary>
    public static readonly TimeSpan PruneInterval = TimeSpan.FromSeconds(60);

    /// <summary>The most rows a view asks for. Past this it says "showing the newest N".</summary>
    public const int QueryLimit = 2_000;

    /// <summary>
    /// What a CLOSE record amounted to. The precedence is the indexer's own
    /// (<c>MftVolumeIndexer.Apply</c>): a delete beats everything, then a rename, then a create.
    /// </summary>
    /// <param name="hadOldName">Whether a RENAME_OLD_NAME record for the same file was captured.
    /// Without one the new name is a move-in from somewhere the map could not resolve, and the
    /// indexer treats it as a fresh entry — so does this.</param>
    public static ChangeKind Classify(uint reason, bool hadOldName)
    {
        if ((reason & NtfsNative.UsnReasonFileDelete) != 0) return ChangeKind.Deleted;
        if ((reason & NtfsNative.UsnReasonRenameNewName) != 0)
            return hadOldName ? ChangeKind.Renamed : ChangeKind.Created;
        if ((reason & NtfsNative.UsnReasonFileCreate) != 0) return ChangeKind.Created;
        return ChangeKind.Modified;
    }

    /// <summary>
    /// True for the data directory and everything under it. Every write to the database is itself a
    /// filesystem change, so without this the log would record its own growth for ever.
    /// </summary>
    public static bool IsExcluded(string pathKey, string excludedRootKey) =>
        string.Equals(pathKey, excludedRootKey, StringComparison.Ordinal) ||
        PathKey.IsUnder(pathKey, excludedRootKey);

    /// <summary>
    /// The lower bound of a range, never earlier than the policy's retention: the writer prunes on
    /// an interval, and a row it has not yet got round to must not outlive the promise the settings
    /// page made. Null when "since the mark" has no mark yet.
    /// </summary>
    public static DateTime? SinceUtc(ChangeRange range, DateTime nowUtc, DateTime? markUtc, ChangeLogPolicy policy)
    {
        var since = SinceUtc(range, nowUtc, markUtc);
        if (since is null || !policy.Enabled) return since;
        var floor = nowUtc - policy.Retention;
        return since < floor ? floor : since;
    }

    /// <summary>The lower bound of a range, or null when "since the mark" has no mark yet.</summary>
    public static DateTime? SinceUtc(ChangeRange range, DateTime nowUtc, DateTime? markUtc) => range switch
    {
        ChangeRange.Last15Minutes => nowUtc.AddMinutes(-15),
        ChangeRange.LastHour => nowUtc.AddHours(-1),
        ChangeRange.Last6Hours => nowUtc.AddHours(-6),
        ChangeRange.Last24Hours => nowUtc.AddHours(-24),
        ChangeRange.SinceMark => markUtc,
        _ => throw new ArgumentOutOfRangeException(nameof(range), range, null),
    };

    /// <summary>
    /// Which thing is missing, in the order the user can do something about it: turn recording on,
    /// then get the helper running, then wait for the build.
    /// </summary>
    /// <param name="indexerRunning">False when the helper declined or died — the client's
    /// "can retry" state — so the banner can offer the retry.</param>
    public static ChangeTimelineAvailability Availability(
        bool recordingOn, bool scoped, bool anyIndexed, bool scopeIndexed, bool isBuilding, bool indexerRunning)
    {
        if (!recordingOn) return ChangeTimelineAvailability.RecordingOff;
        if (!indexerRunning) return ChangeTimelineAvailability.IndexerUnavailable;

        var covered = scoped ? scopeIndexed : anyIndexed;
        if (covered) return ChangeTimelineAvailability.Ready;
        if (isBuilding) return ChangeTimelineAvailability.Building;
        return scoped ? ChangeTimelineAvailability.ScopeNotIndexed : ChangeTimelineAvailability.IndexerUnavailable;
    }

    /// <summary>
    /// The folder a timeline opened on <paramref name="path"/> should watch: the path itself, or
    /// for somewhere inside an archive the archive's own folder. An entry in a container has no
    /// real path, so a query scoped to it would match nothing and read as "nothing changed" —
    /// the misleading answer this view exists to avoid. Null for no path, meaning every drive.
    /// </summary>
    public static string? ScopeFor(string? path, Func<string, bool> isArchiveFile)
    {
        if (path is not { Length: > 0 }) return null;
        return Archives.ArchivePath.Parse(path, isArchiveFile) is { } inside
            ? Path.GetDirectoryName(inside.ArchiveFile)
            : path;
    }

    /// <summary>Why the list is empty, said with the range and scope in it so it reads as a fact
    /// rather than a failure.</summary>
    public static string EmptyMessage(ChangeRange range, bool scoped) =>
        $"Nothing changed {(scoped ? "here" : "on this PC")} {RangePhrase(range)}.";

    /// <summary>"in the last hour", "since the mark" — the range as a phrase.</summary>
    public static string RangePhrase(ChangeRange range) => range switch
    {
        ChangeRange.Last15Minutes => "in the last 15 minutes",
        ChangeRange.LastHour => "in the last hour",
        ChangeRange.Last6Hours => "in the last 6 hours",
        ChangeRange.Last24Hours => "in the last 24 hours",
        ChangeRange.SinceMark => "since the mark",
        _ => throw new ArgumentOutOfRangeException(nameof(range), range, null),
    };
}
