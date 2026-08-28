namespace BertBrowser.Core.Services;

/// <summary>
/// Throughput and time-remaining as words, beside <see cref="ByteSizeFormatter"/> and reusing it
/// for the byte half so "112 MB/s" and "112 MB" round the same way.
/// </summary>
public static class RateFormatter
{
    /// <summary>e.g. "112 MB/s". Empty when there is no figure yet — a blank is honest where a
    /// zero would read as a stall.</summary>
    public static string Speed(double? bytesPerSecond)
    {
        if (bytesPerSecond is not { } rate || rate <= 0) return "";
        return $"{ByteSizeFormatter.Format((long)Math.Round(rate))}/s";
    }

    /// <summary>
    /// e.g. "about 6 minutes left". Deliberately vague: a transfer's remaining time is an estimate
    /// off a smoothed rate, and "about 6 minutes" is both truer and easier to read than "5:47".
    /// </summary>
    public static string Remaining(TimeSpan? remaining)
    {
        if (remaining is not { } left) return "";
        if (left < TimeSpan.FromMinutes(1)) return "less than a minute left";

        // Rounded to minutes once, then split — rounding after the split lets 1 h 59 m 50 s come
        // out as "1 hour 60 minutes".
        var totalMinutes = (int)Math.Round(left.TotalMinutes);
        if (totalMinutes < 60) return $"about {Plural(totalMinutes, "minute")} left";

        var hours = totalMinutes / 60;
        var trailing = totalMinutes % 60;
        return trailing == 0
            ? $"about {Plural(hours, "hour")} left"
            : $"about {Plural(hours, "hour")} {Plural(trailing, "minute")} left";
    }

    private static string Plural(int count, string unit) =>
        count == 1 ? $"1 {unit}" : $"{count} {unit}s";
}
