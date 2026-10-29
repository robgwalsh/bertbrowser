using BertBrowser.Core.Services.Diff;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class TextDifferTests
{
    private static TextDiff Diff(string left, string right, DiffOptions? options = null) =>
        TextDiffer.Compare(Lines(left), Lines(right), options);

    private static string[] Lines(string text) =>
        text.Length == 0 ? [] : text.Split('\n');

    /// <summary>A compact rendering of the rows, so a test reads as what the view would show.</summary>
    private static string Render(TextDiff diff) =>
        string.Join(" ", diff.Rows.Select(r => r.Op switch
        {
            DiffOp.Equal => $"={r.LeftLine}/{r.RightLine}",
            DiffOp.Replace => $"~{r.LeftLine}/{r.RightLine}",
            DiffOp.Delete => $"-{r.LeftLine}",
            _ => $"+{r.RightLine}",
        }));

    // --- the shape of an answer ---

    [Fact]
    public void IdenticalTextsAreAllEqualAndHaveNoHunks()
    {
        var diff = Diff("a\nb\nc", "a\nb\nc");

        Assert.Equal("=1/1 =2/2 =3/3", Render(diff));
        Assert.Empty(diff.Hunks);
        Assert.True(diff.Identical);
        Assert.False(diff.BudgetExceeded);
    }

    [Fact]
    public void TwoEmptyTextsAreIdentical()
    {
        var diff = Diff("", "");

        Assert.Empty(diff.Rows);
        Assert.True(diff.Identical);
    }

    [Fact]
    public void AOneLineChangeIsOneReplaceAndOneHunk()
    {
        var diff = Diff("a\nb\nc", "a\nB\nc");

        Assert.Equal("=1/1 ~2/2 =3/3", Render(diff));
        Assert.Equal(1, diff.ChangedLineCount);

        var hunk = Assert.Single(diff.Hunks);
        Assert.Equal(1, hunk.FirstRow);
        Assert.Equal(1, hunk.RowCount);
    }

    [Fact]
    public void APureInsertionLeavesTheLeftBlank()
    {
        var diff = Diff("a\nc", "a\nb\nc");

        Assert.Equal("=1/1 +2 =2/3", Render(diff));
    }

    [Fact]
    public void APureDeletionLeavesTheRightBlank()
    {
        var diff = Diff("a\nb\nc", "a\nc");

        Assert.Equal("=1/1 -2 =3/2", Render(diff));
    }

    [Fact]
    public void AnInsertionAtTheTopIsAHunkAtRowZero()
    {
        var diff = Diff("a", "z\na");

        Assert.Equal("+1 =1/2", Render(diff));
        Assert.Equal(0, Assert.Single(diff.Hunks).FirstRow);
    }

    [Fact]
    public void AnInsertionAtTheEndIsAHunkAtTheEnd()
    {
        var diff = Diff("a", "a\nz");

        Assert.Equal("=1/1 +2", Render(diff));
    }

    [Fact]
    public void EverythingDeletedIsAllDeletions()
    {
        var diff = Diff("a\nb", "");

        Assert.Equal("-1 -2", Render(diff));
    }

    [Fact]
    public void EverythingInsertedIsAllInsertions()
    {
        var diff = Diff("", "a\nb");

        Assert.Equal("+1 +2", Render(diff));
    }

    // --- the pairing, which is the piece most likely to be wrong ---

    /// <summary>
    /// Without pairing this renders as three blank-right rows then three blank-left rows, and the
    /// reader has to work out which line became which.
    /// </summary>
    [Fact]
    public void ARunOfDeletionsBesideARunOfInsertionsIsPairedIntoReplaces()
    {
        var diff = Diff("a\n1\n2\n3\nz", "a\nX\nY\nZ\nz");

        Assert.Equal("=1/1 ~2/2 ~3/3 ~4/4 =5/5", Render(diff));
    }

    /// <summary>
    /// Four lines becoming two is two replacements and two deletions — not four of anything. The
    /// remainder of the longer run has to stay honest about which side it is on.
    /// </summary>
    [Fact]
    public void AnUnequalRunPadsTheShorterSide()
    {
        var diff = Diff("a\n1\n2\n3\n4\nz", "a\nX\nY\nz");

        Assert.Equal("=1/1 ~2/2 ~3/3 -4 -5 =6/4", Render(diff));
    }

    [Fact]
    public void MoreInsertionsThanDeletionsPadsTheLeft()
    {
        var diff = Diff("a\n1\nz", "a\nX\nY\nZ\nz");

        Assert.Equal("=1/1 ~2/2 +3 +4 =3/5", Render(diff));
    }

    // --- hunk grouping ---

    [Fact]
    public void ChangesFurtherApartThanTwiceTheContextAreSeparateHunks()
    {
        var left = string.Join("\n", ["X", .. Enumerable.Repeat("same", 20), "Y"]);
        var right = string.Join("\n", ["1", .. Enumerable.Repeat("same", 20), "2"]);

        Assert.Equal(2, Diff(left, right).Hunks.Count);
    }

    [Fact]
    public void ChangesCloserThanTwiceTheContextAreOneHunk()
    {
        var left = "X\nsame\nsame\nY";
        var right = "1\nsame\nsame\n2";

        Assert.Single(Diff(left, right).Hunks);
    }

    [Fact]
    public void EveryHunkPointsAtARealChangedRow()
    {
        var diff = Diff("a\nX\nb\nc\nd\ne\nf\ng\nh\ni\nj\nY\nk", "a\n1\nb\nc\nd\ne\nf\ng\nh\ni\nj\n2\nk");

        Assert.NotEmpty(diff.Hunks);
        foreach (var hunk in diff.Hunks)
        {
            Assert.NotEqual(DiffOp.Equal, diff.Rows[hunk.FirstRow].Op);
        }
    }

    // --- the options ---

    /// <summary>
    /// On by default, because a difference of trailing spaces is the one difference a person cannot
    /// see and will not believe.
    /// </summary>
    [Fact]
    public void TrailingWhitespaceIsIgnoredByDefault() =>
        Assert.True(Diff("a  \nb", "a\nb").Identical);

    [Fact]
    public void TrailingWhitespaceCanBeMadeToMatter() =>
        Assert.False(Diff("a  \nb", "a\nb", new DiffOptions(IgnoreTrailingWhitespace: false)).Identical);

    [Fact]
    public void LeadingWhitespaceStillMattersByDefault() =>
        Assert.False(Diff("    a", "a").Identical);

    [Fact]
    public void IgnoreAllWhitespaceReachesIndentation() =>
        Assert.True(Diff("    a", "a", new DiffOptions(IgnoreAllWhitespace: true)).Identical);

    [Fact]
    public void IgnoreCaseDoesWhatItSays()
    {
        Assert.False(Diff("Hello", "hello").Identical);
        Assert.True(Diff("Hello", "hello", new DiffOptions(IgnoreCase: true)).Identical);
    }

    // --- the budget ---

    /// <summary>
    /// Two files sharing nothing are further apart than the budget allows. The answer is a
    /// positional pairing and a flag saying so — never an exception, and never an empty result,
    /// which a view would have nothing to show for.
    /// </summary>
    [Fact]
    public void TwoWhollyDifferentFilesFallBackToPositionalAlignment()
    {
        var left = string.Join("\n", Enumerable.Range(0, 3000).Select(i => $"left {i}"));
        var right = string.Join("\n", Enumerable.Range(0, 3000).Select(i => $"right {i}"));

        var diff = Diff(left, right);

        Assert.True(diff.BudgetExceeded);
        Assert.Equal(3000, diff.Rows.Count);
        Assert.All(diff.Rows, r => Assert.Equal(DiffOp.Replace, r.Op));
        Assert.NotEmpty(diff.Hunks);
    }

    [Fact]
    public void AFileLongerThanTheLineCapFallsBackRatherThanRunning()
    {
        var many = new string[TextDiffer.MaxLines + 1];
        Array.Fill(many, "x");

        Assert.True(TextDiffer.Compare(many, ["x"]).BudgetExceeded);
    }

    /// <summary>
    /// The reduction that makes this practical: one changed line in a large file is a tiny edit
    /// script, however large the file, because the matching ends are trimmed before Myers starts.
    /// </summary>
    [Fact]
    public void ALargeFileWithOneChangedLineStillAligns()
    {
        var left = Enumerable.Range(0, 50_000).Select(i => $"line {i}").ToArray();
        var right = (string[])left.Clone();
        right[25_000] = "changed";

        var diff = TextDiffer.Compare(left, right);

        Assert.False(diff.BudgetExceeded);
        Assert.Equal(1, diff.ChangedLineCount);
        Assert.Single(diff.Hunks);
    }

    // --- what a row means ---

    [Fact]
    public void EveryRowNumbersOnlyTheSidesItHas()
    {
        var diff = Diff("a\nb\nc", "a\nx");

        foreach (var row in diff.Rows)
        {
            switch (row.Op)
            {
                case DiffOp.Equal or DiffOp.Replace:
                    Assert.True(row.LeftLine > 0 && row.RightLine > 0);
                    break;
                case DiffOp.Delete:
                    Assert.True(row.LeftLine > 0 && row.RightLine == 0);
                    break;
                default:
                    Assert.True(row.LeftLine == 0 && row.RightLine > 0);
                    break;
            }
        }
    }

    /// <summary>Line numbers must run 1, 2, 3 down each side with no gaps and no repeats — they are
    /// the gutter a reader checks the diff against.</summary>
    [Fact]
    public void LineNumbersAreConsecutiveDownEachSide()
    {
        var diff = Diff("a\n1\n2\nb\nc", "a\nX\nb\nY\nc");

        var left = diff.Rows.Where(r => r.LeftLine > 0).Select(r => r.LeftLine);
        var right = diff.Rows.Where(r => r.RightLine > 0).Select(r => r.RightLine);

        Assert.Equal(Enumerable.Range(1, 5), left);
        Assert.Equal(Enumerable.Range(1, 5), right);
    }
}
