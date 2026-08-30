using System.Globalization;

namespace BertBrowser.Core.Services.Search;

/// <summary>
/// Resolves the date literals a <c>dm:</c> filter accepts — "today", "thisweek", "2026-08",
/// "2026-08-29" — into a half-open UTC range.
/// </summary>
/// <remarks>
/// <para><strong>The clock is a parameter, never read here.</strong> Same reason
/// <c>TransferRate</c> takes its timestamps as arguments: it is what lets the tests pin
/// "today" instead of racing midnight.</para>
/// <para><strong>Shorthands resolve in local time, and the range comes back in UTC.</strong>
/// The Modified column shows local time and <c>{modified}</c> in a rename stamps local time,
/// so "today" has to mean the user's day; but <c>fs_entry.modified_utc</c> stores UTC, so the
/// bounds are converted before they are compared. Getting this backwards shifts every result
/// by the UTC offset, which is invisible in London and eight hours wrong elsewhere.</para>
/// </remarks>
public static class DateShorthand
{
    /// <summary>Weeks start Monday (ISO 8601) rather than following the current culture, so a
    /// query means the same thing on every machine and the tests can assert it.</summary>
    private const DayOfWeek WeekStart = DayOfWeek.Monday;

    /// <summary>
    /// Resolves <paramref name="text"/> against <paramref name="nowLocal"/> into the half-open
    /// local range [lo, hi), converted to UTC. Returns false when the text is not a date.
    /// </summary>
    public static bool TryResolve(string? text, DateTime nowLocal, out DateTime loUtc, out DateTime hiUtc)
    {
        loUtc = default;
        hiUtc = default;
        if (string.IsNullOrWhiteSpace(text)) return false;

        if (!TryResolveLocal(text.Trim(), nowLocal, out var lo, out var hi)) return false;

        loUtc = ToUtc(lo);
        hiUtc = ToUtc(hi);
        return true;
    }

    /// <summary>A local wall-clock bound to UTC. Unspecified kinds are local by construction here.</summary>
    private static DateTime ToUtc(DateTime local) =>
        DateTime.SpecifyKind(local, DateTimeKind.Local).ToUniversalTime();

    private static bool TryResolveLocal(string text, DateTime nowLocal, out DateTime lo, out DateTime hi)
    {
        var today = nowLocal.Date;
        lo = default;
        hi = default;

        switch (text.ToUpperInvariant())
        {
            case "TODAY":
                lo = today; hi = today.AddDays(1); return true;
            case "YESTERDAY":
                lo = today.AddDays(-1); hi = today; return true;
            case "THISWEEK":
                lo = StartOfWeek(today); hi = lo.AddDays(7); return true;
            case "LASTWEEK":
                hi = StartOfWeek(today); lo = hi.AddDays(-7); return true;
            case "THISMONTH":
                lo = new DateTime(today.Year, today.Month, 1); hi = lo.AddMonths(1); return true;
            case "LASTMONTH":
                hi = new DateTime(today.Year, today.Month, 1); lo = hi.AddMonths(-1); return true;
            case "THISYEAR":
                lo = new DateTime(today.Year, 1, 1); hi = lo.AddYears(1); return true;
            case "LASTYEAR":
                hi = new DateTime(today.Year, 1, 1); lo = hi.AddYears(-1); return true;
        }

        // "last7days" / "last30days" / "last12months" — a window ending at the end of today,
        // so an item modified an hour ago is inside "last7days".
        if (TryParseLastWindow(text, today, out lo, out hi)) return true;

        // "2026-08-29", "2026-08", "2026" — a calendar span, most specific first.
        if (DateTime.TryParseExact(text, "yyyy-MM-dd", CultureInfo.InvariantCulture,
                DateTimeStyles.None, out var day))
        {
            lo = day; hi = day.AddDays(1); return true;
        }
        if (DateTime.TryParseExact(text, "yyyy-MM", CultureInfo.InvariantCulture,
                DateTimeStyles.None, out var month))
        {
            lo = month; hi = month.AddMonths(1); return true;
        }
        if (text.Length == 4 && int.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var year)
            && year is >= 1601 and <= 9999)
        {
            lo = new DateTime(year, 1, 1); hi = lo.AddYears(1); return true;
        }

        return false;
    }

    /// <summary>"LAST&lt;n&gt;DAYS" / "LAST&lt;n&gt;WEEKS" / "LAST&lt;n&gt;MONTHS" / "LAST&lt;n&gt;YEARS".</summary>
    private static bool TryParseLastWindow(string text, DateTime today, out DateTime lo, out DateTime hi)
    {
        lo = default;
        hi = default;

        var upper = text.ToUpperInvariant();
        if (!upper.StartsWith("LAST", StringComparison.Ordinal)) return false;

        var rest = upper[4..];
        var digits = 0;
        while (digits < rest.Length && char.IsAsciiDigit(rest[digits])) digits++;
        if (digits == 0) return false;

        if (!int.TryParse(rest[..digits], NumberStyles.None, CultureInfo.InvariantCulture, out var n) || n <= 0)
            return false;

        // The window ends at the end of today; "last1days" is therefore today itself.
        hi = today.AddDays(1);
        lo = rest[digits..] switch
        {
            "DAY" or "DAYS" => hi.AddDays(-n),
            "WEEK" or "WEEKS" => hi.AddDays(-7 * n),
            "MONTH" or "MONTHS" => hi.AddMonths(-n),
            "YEAR" or "YEARS" => hi.AddYears(-n),
            _ => default,
        };
        return lo != default;
    }

    private static DateTime StartOfWeek(DateTime day)
    {
        var delta = (7 + (day.DayOfWeek - WeekStart)) % 7;
        return day.AddDays(-delta);
    }
}
