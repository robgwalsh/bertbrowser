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
}
