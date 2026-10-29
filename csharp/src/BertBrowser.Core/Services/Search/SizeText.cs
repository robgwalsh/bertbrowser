using System.Globalization;

namespace BertBrowser.Core.Services.Search;

/// <summary>
/// Parses the size literals a <c>size:</c> filter accepts — "100mb", "1.5g", "512", "0".
/// </summary>
/// <remarks>
/// Multiples are <strong>1024-based</strong>, and that is not a style choice: it is what
/// <see cref="ByteSizeFormatter"/> uses, so it is what the Size column shows. A 1000-based
/// "mb" would make <c>size:&gt;100mb</c> return rows the list labels "99.9 MB", and a filter
/// disagreeing with the column beside it reads as a bug in the filter.
/// </remarks>
public static class SizeText
{
    /// <summary>Unit suffixes, longest first so "kb" is tried before "k".</summary>
    private static readonly (string Suffix, long Multiple)[] Units =
    {
        ("PB", 1L << 50), ("TB", 1L << 40), ("GB", 1L << 30), ("MB", 1L << 20), ("KB", 1L << 10),
        ("P",  1L << 50), ("T",  1L << 40), ("G",  1L << 30), ("M",  1L << 20), ("K",  1L << 10),
        ("B",  1L),
    };

    /// <summary>
    /// Parses "100mb" into a byte count. Returns false for anything unusable — a bare unit,
    /// a negative number, a value that overflows, or trailing rubbish.
    /// </summary>
    public static bool TryParse(string? text, out long bytes)
    {
        bytes = 0;
        if (string.IsNullOrWhiteSpace(text)) return false;

        var trimmed = text.Trim().ToUpperInvariant();

        var multiple = 1L;
        foreach (var (suffix, value) in Units)
        {
            if (!trimmed.EndsWith(suffix, StringComparison.Ordinal)) continue;
            multiple = value;
            trimmed = trimmed[..^suffix.Length].TrimEnd();
            break;
        }

        // A bare unit ("mb") names no quantity.
        if (trimmed.Length == 0) return false;

        if (!double.TryParse(trimmed, NumberStyles.AllowDecimalPoint, CultureInfo.InvariantCulture, out var number))
            return false;
        if (number < 0 || double.IsNaN(number) || double.IsInfinity(number)) return false;

        var scaled = number * multiple;
        if (scaled > long.MaxValue) return false;

        bytes = (long)scaled;
        return true;
    }
}
