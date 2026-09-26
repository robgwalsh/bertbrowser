using System.Globalization;

namespace BertBrowser.Core.Services;

/// <summary>
/// "3 min ago" for a timeline that is read while it is still happening. Past a day an age stops
/// being useful and the local date takes over, in the same <c>"g"</c> form every other timestamp in
/// the app uses.
/// </summary>
public static class RelativeTime
{
    public static string Format(DateTime utc, DateTime nowUtc)
    {
        var age = nowUtc - utc;
        // A record stamped by a clock slightly ahead of ours is not from the future.
        if (age < TimeSpan.FromMinutes(1)) return "just now";
        if (age < TimeSpan.FromHours(1)) return $"{(int)age.TotalMinutes} min ago";
        if (age < TimeSpan.FromDays(1)) return $"{(int)age.TotalHours} h ago";
        return utc.ToLocalTime().ToString("g");
    }

    /// <summary>
    /// A timestamp by the day it fell on, for a list read at leisure rather than a live timeline:
    /// "Today 14:05", "Yesterday 09:30", then the date. Null reads as <paramref name="missing"/> —
    /// "Never" for a saved search or workspace that has not been used.
    /// </summary>
    /// <remarks>Both arguments are local time; the caller converts, so this stays a pure function of
    /// what it is given and the test does not depend on the machine's time zone.</remarks>
    public static string Day(DateTime? local, DateTime nowLocal, string missing = "Never")
    {
        if (local is not { } when) return missing;
        var days = (nowLocal.Date - when.Date).Days;
        return days switch
        {
            0 => $"Today {when:HH:mm}",
            1 => $"Yesterday {when:HH:mm}",
            _ when when.Year == nowLocal.Year => when.ToString("d MMM HH:mm", CultureInfo.InvariantCulture),
            _ => when.ToString("d MMM yyyy", CultureInfo.InvariantCulture),
        };
    }
}
