namespace BertBrowser.Core.Services.Columns;

/// <summary>
/// One shell property as a column shows it: the string to render, plus a typed key to order by when
/// the property system gave one.
/// </summary>
/// <param name="Display">
/// Always populated — <c>PSFormatForDisplay</c>'s output, which is what the cell renders and what a
/// person recognises ("6240 x 4160", "00:03:41", "192kbps").
/// </param>
/// <param name="Number">
/// Set for the numeric variant types. At most one of this and <paramref name="DateUtc"/> is set;
/// both null means the display string is all there is.
/// </param>
/// <param name="DateUtc">
/// Set for <c>VT_FILETIME</c>. This is the one that has to exist: a formatted date sorts by
/// day-of-month under any string comparison, and Date taken is the first column anyone adds.
/// </param>
public sealed record ColumnValue(string Display, double? Number = null, DateTime? DateUtc = null);

/// <summary>
/// How two values in the same column order against each other.
/// </summary>
/// <remarks>
/// <para>
/// The text comparer is a parameter rather than something this class reaches for, because natural
/// ordering is <c>StrCmpLogicalW</c> — a <c>shlwapi</c> call living in the App. Taking it as an
/// argument is what makes this testable in a project with no UI reference, the same reason
/// <c>TransferRate</c> takes its timestamps.
/// </para>
/// </remarks>
public static class ColumnComparison
{
    /// <summary>
    /// Orders two cell values, with anything unknown last.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>A null sorts last in both directions</b>, which is why it is answered here rather than
    /// left to the caller's descending flip. A metadata column fills in as rows are read, and a
    /// blank is not a small value — it is the absence of one, and floating a screenful of blanks to
    /// the top the moment someone reverses the sort would be indistinguishable from a bug.
    /// </para>
    /// <para>
    /// Typed keys beat text, and a column where the two disagree — one file's handler returning a
    /// number where the rest return words — degrades to comparing the display strings rather than
    /// ordering half the list by one rule and half by another.
    /// </para>
    /// </remarks>
    public static int Compare(ColumnValue? a, ColumnValue? b, IComparer<string?> text)
    {
        if (a is null) return b is null ? 0 : 1;
        if (b is null) return -1;

        if (a.DateUtc is { } left && b.DateUtc is { } right) return left.CompareTo(right);
        if (a.Number is { } x && b.Number is { } y) return x.CompareTo(y);

        return text.Compare(a.Display, b.Display);
    }

    /// <summary>Where a value sorts relative to a missing one: 0 for something, 1 for nothing. The
    /// file list applies this as a band <em>above</em> the ascending/descending flip.</summary>
    public static int KnownBand(ColumnValue? value) => value is null ? 1 : 0;
}
