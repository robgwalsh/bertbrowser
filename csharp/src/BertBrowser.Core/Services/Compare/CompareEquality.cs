namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// How far apart two timestamps may be and still count as the same instant.
/// </summary>
/// <param name="Slack">Two seconds, because FAT and exFAT store a write time to two-second
/// granularity: a file copied from NTFS onto a USB stick comes back with a timestamp up to two
/// seconds off and would otherwise read as "older" for ever, on every pass, for every file.</param>
/// <param name="AllowWholeHourShift">Also forgive a gap of a whole hour or two, to within
/// <paramref name="Slack"/>. FAT stores local time with no zone, so every file on the stick appears
/// to shift by an hour when the clocks change.</param>
/// <remarks>
/// The hour rule is deliberately a <b>whole-hour</b> rule and not a one-hour band. A blanket ±1 h
/// tolerance would call a file that really is fifty minutes newer "the same", which is a wrong
/// answer in the dangerous direction — "same" is what authorises deleting the other side.
/// </remarks>
public readonly record struct CompareTolerance(TimeSpan Slack, bool AllowWholeHourShift)
{
    /// <summary>Filesystem granularity only. The default, and what an unrecognised volume gets.</summary>
    public static readonly CompareTolerance Strict = new(CompareEquality.FatGranularity, false);

    /// <summary>Granularity plus the whole-hour shift, for a FAT-family volume.</summary>
    public static readonly CompareTolerance Loose = new(CompareEquality.FatGranularity, true);

    /// <summary>
    /// The tolerance for a pair of volumes, from <see cref="DriveInfo.DriveFormat"/> on each side.
    /// </summary>
    /// <remarks>
    /// Takes the format strings rather than reading <see cref="DriveInfo"/> itself, so the rule
    /// stays pure and testable. <b>Fails closed to <see cref="Strict"/></b> when a format is null
    /// or unrecognised — a UNC path, a mapped drive, a volume that would not answer. An unforgiven
    /// hour shows as "newer" and offers a copy, which is additive and harmless; a wrongly forgiven
    /// one shows as "same" and offers a delete.
    /// </remarks>
    public static CompareTolerance For(string? leftFormat, string? rightFormat) =>
        IsFatFamily(leftFormat) || IsFatFamily(rightFormat) ? Loose : Strict;

    private static bool IsFatFamily(string? format) =>
        format is not null &&
        (format.StartsWith("FAT", StringComparison.OrdinalIgnoreCase) ||
         format.Equals("exFAT", StringComparison.OrdinalIgnoreCase));
}

/// <summary>
/// Deciding whether two entries are the same file. Pure, and the only place the definition lives.
/// </summary>
public static class CompareEquality
{
    /// <summary>FAT and exFAT round a write time to this.</summary>
    public static readonly TimeSpan FatGranularity = TimeSpan.FromSeconds(2);

    private static readonly TimeSpan[] ForgivenShifts =
        [TimeSpan.FromHours(1), TimeSpan.FromHours(2)];

    /// <summary>
    /// -1, 0 or +1, with zero meaning "the same instant as far as this tolerance can tell" — so
    /// "newer" always means <em>measurably</em> newer.
    /// </summary>
    public static int CompareTimes(DateTime a, DateTime b, CompareTolerance tolerance)
    {
        var gap = a - b;
        var magnitude = gap.Duration();

        if (magnitude <= tolerance.Slack) return 0;

        if (tolerance.AllowWholeHourShift)
        {
            foreach (var shift in ForgivenShifts)
            {
                if ((magnitude - shift).Duration() <= tolerance.Slack) return 0;
            }
        }

        return gap > TimeSpan.Zero ? 1 : -1;
    }

    /// <summary>
    /// The verdict for one relative path. A null side means the entry is absent there.
    /// </summary>
    /// <remarks>
    /// Two directories always come back <see cref="CompareVerdict.Same"/>: a folder has no size
    /// worth comparing and its own timestamp moves for reasons nobody cares about, so its real
    /// verdict is the roll-up of everything beneath it, which
    /// <see cref="FolderComparer.Compare"/> applies afterwards. <see cref="CompareVerdict.Same"/>
    /// is the correct seed for that fold, and is also the right answer for two empty folders.
    /// </remarks>
    public static CompareVerdict Verdict(CompareEntry? left, CompareEntry? right, CompareTolerance tolerance)
    {
        if (left is not { } l) return right is null ? CompareVerdict.Unknown : CompareVerdict.RightOnly;
        if (right is not { } r) return CompareVerdict.LeftOnly;

        if (l.IsDirectory != r.IsDirectory) return CompareVerdict.Differs;
        if (l.IsDirectory) return CompareVerdict.Same;

        // The index's name-only build path writes MinValue for every entry on the volume. Comparing
        // sizes alone there would call a whole drive "the same" and authorise deleting it.
        if (l.ModifiedUtc == default || r.ModifiedUtc == default) return CompareVerdict.Unknown;

        var byTime = CompareTimes(l.ModifiedUtc, r.ModifiedUtc, tolerance);
        if (byTime > 0) return CompareVerdict.LeftNewer;
        if (byTime < 0) return CompareVerdict.RightNewer;

        // Same instant. Only the size can still separate them, and when it does neither side can be
        // called newer — which is exactly what Differs is for.
        return l.SizeBytes == r.SizeBytes ? CompareVerdict.Same : CompareVerdict.Differs;
    }
}
