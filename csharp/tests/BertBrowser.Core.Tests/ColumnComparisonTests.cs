using BertBrowser.Core.Services.Columns;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ColumnComparisonTests
{
    /// <summary>
    /// Stands in for <c>NaturalStringComparer</c>, which is a shlwapi call in the App. Ordinal is
    /// enough to show <em>which arm</em> was taken, which is the whole of what this rule decides.
    /// </summary>
    private static readonly IComparer<string?> Text = StringComparer.Ordinal;

    private static ColumnValue Words(string display) => new(display);

    private static ColumnValue Number(string display, double value) => new(display, Number: value);

    private static ColumnValue Date(string display, DateTime value) => new(display, DateUtc: value);

    [Fact]
    public void ADateIsComparedAsADateAndNotAsItsText()
    {
        // The case the typed key exists for. As text, "31/08/2026" precedes "01/09/2026" under
        // every string comparison there is, so a Date taken column would sort by day of month.
        var august = Date("31/08/2026 14:03", new DateTime(2026, 8, 31, 14, 3, 0, DateTimeKind.Utc));
        var september = Date("01/09/2026 09:00", new DateTime(2026, 9, 1, 9, 0, 0, DateTimeKind.Utc));

        Assert.True(ColumnComparison.Compare(august, september, Text) < 0);
        Assert.True(Text.Compare(august.Display, september.Display) > 0);
    }

    [Fact]
    public void ANumberIsComparedAsANumberAndNotAsItsText()
    {
        // Grouping separators are what break the string comparison here — "1,024 kbps" against
        // "128 kbps" compares '1' with '1' then ',' with '2'.
        var faster = Number("1,024 kbps", 1024);
        var slower = Number("128 kbps", 128);

        Assert.True(ColumnComparison.Compare(slower, faster, Text) < 0);
        Assert.True(Text.Compare(slower.Display, faster.Display) > 0);
    }

    [Fact]
    public void AnythingElseFallsBackToTheDisplayText() =>
        Assert.True(ColumnComparison.Compare(Words("Canon"), Words("Nikon"), Text) < 0);

    [Fact]
    public void AColumnWhoseHandlersDisagreeAboutTypeComparesAsText()
    {
        // One file's handler returning a number where the rest return words. Ordering half the list
        // by one rule and half by another would be worse than ordering all of it by text.
        var typed = Number("12", 12);
        var untyped = Words("9");

        Assert.Equal(Text.Compare("12", "9"), ColumnComparison.Compare(typed, untyped, Text));
    }

    // --- Unknown sinks last ---

    [Fact]
    public void AMissingValueSortsAfterOneThatIsThere()
    {
        Assert.True(ColumnComparison.Compare(null, Words("anything"), Text) > 0);
        Assert.True(ColumnComparison.Compare(Words("anything"), null, Text) < 0);
    }

    [Fact]
    public void TwoMissingValuesAreEqual() =>
        Assert.Equal(0, ColumnComparison.Compare(null, null, Text));

    /// <summary>
    /// The half that makes "unknown sinks last" true in <em>both</em> directions. A blank is the
    /// absence of a value, not a small one — floating a screenful of blanks to the top the moment
    /// someone reverses the sort is indistinguishable from a bug, so the band is applied above the
    /// direction flip rather than being left to <see cref="ColumnComparison.Compare"/>.
    /// </summary>
    [Fact]
    public void TheKnownBandIsWhatKeepsBlanksDownUnderADescendingSort()
    {
        Assert.Equal(0, ColumnComparison.KnownBand(Words("something")));
        Assert.Equal(1, ColumnComparison.KnownBand(null));
    }
}
