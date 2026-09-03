namespace BertBrowser.Core.Services.Changes;

/// <summary>
/// Whether the USN tail writes a change log at all, and how long rows live.
/// </summary>
/// <remarks>
/// <para>
/// <b>The default is off.</b> A log of every file created, changed, deleted or renamed on every
/// drive is sensitive, and it sits in a plain SQLite file in the profile folder; nothing is
/// recorded until the user turns it on in Settings, and turning it off wipes what was kept.
/// </para>
/// <para>
/// Retention is one of a short menu of hours rather than a free number, because the same value
/// crosses the pipe to the elevated helper as the argument of one verb (<c>Record</c>), and the
/// helper's whole security posture rests on accepting nothing it cannot validate exhaustively.
/// <c>0</c> is off. See <c>IndexProtocol</c>.
/// </para>
/// </remarks>
public readonly record struct ChangeLogPolicy(bool Enabled, TimeSpan Retention)
{
    /// <summary>The choices the settings page offers, in hours.</summary>
    public static readonly IReadOnlyList<int> RetentionOptions = [1, 6, 24, 168];

    public const int DefaultRetentionHours = 24;

    public static ChangeLogPolicy Off => new(false, TimeSpan.Zero);

    /// <summary>True for 0 (off) and for each of <see cref="RetentionOptions"/>; nothing else.</summary>
    public static bool IsAcceptableHours(int hours) =>
        hours == 0 || RetentionOptions.Contains(hours);

    /// <summary>The wire and settings form: 0 is off, otherwise one of the menu's hours.</summary>
    public static ChangeLogPolicy FromHours(int hours)
    {
        if (!IsAcceptableHours(hours))
            throw new ArgumentOutOfRangeException(nameof(hours), hours, "Not one of the retention options.");
        return hours == 0 ? Off : new ChangeLogPolicy(true, TimeSpan.FromHours(hours));
    }

    public int ToHours() => Enabled ? (int)Retention.TotalHours : 0;
}
