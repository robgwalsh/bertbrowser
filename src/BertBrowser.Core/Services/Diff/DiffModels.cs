namespace BertBrowser.Core.Services.Diff;

/// <summary>What one displayed row of a side-by-side diff is.</summary>
public enum DiffOp
{
    /// <summary>The same line on both sides.</summary>
    Equal,

    /// <summary>On the left only; the right cell is blank.</summary>
    Delete,

    /// <summary>On the right only; the left cell is blank.</summary>
    Insert,

    /// <summary>A line on each side, paired: this line became that one.</summary>
    Replace,
}

/// <summary>
/// One row of the side-by-side view.
/// </summary>
/// <param name="LeftLine">1-based line number, or 0 for "nothing on this side".</param>
/// <param name="RightLine">1-based line number, or 0 for "nothing on this side".</param>
/// <remarks>
/// Line numbers rather than text, so the rows for a large file cost a few bytes each and the view
/// reads the strings it already has. The pairing has already happened by the time these exist: a
/// run of deletions beside a run of insertions has been folded into <see cref="DiffOp.Replace"/>
/// rows, so changed lines sit opposite each other instead of stacking.
/// </remarks>
public readonly record struct DiffRow(DiffOp Op, int LeftLine, int RightLine);

/// <summary>
/// A run of changed rows with the equal rows either side of it left out.
/// </summary>
/// <param name="FirstRow">Index into <see cref="TextDiff.Rows"/> — so prev/next-difference is a
/// scroll to a row the view already has, not a search.</param>
public readonly record struct DiffHunk(int FirstRow, int RowCount, int LeftLine, int RightLine);

/// <summary>What comparing two texts found.</summary>
/// <param name="ChangedLineCount">Rows that are not <see cref="DiffOp.Equal"/>.</param>
/// <param name="BudgetExceeded">
/// The two texts were too far apart to align within the work budget, so
/// <see cref="Rows"/> is a plain positional pairing rather than an edit script. Said out loud
/// because it changes what the rows mean, and a view that did not say so would be presenting a
/// guess as an answer.
/// </param>
public sealed record TextDiff(
    IReadOnlyList<DiffRow> Rows,
    IReadOnlyList<DiffHunk> Hunks,
    int LeftLineCount,
    int RightLineCount,
    int ChangedLineCount,
    bool BudgetExceeded)
{
    public bool Identical => ChangedLineCount == 0;
}

/// <summary>
/// What counts as the same line.
/// </summary>
/// <param name="IgnoreTrailingWhitespace">
/// On by default. Line endings are already normalised by the reader, and a difference of trailing
/// spaces is the one difference a person cannot see and will not believe.
/// </param>
/// <remarks>
/// Deliberately not offered: ignoring blank lines, which desynchronises the gutter numbers people
/// are reading off, and anything language-aware, which would make the answer depend on a guess
/// about the file's type.
/// </remarks>
public sealed record DiffOptions(
    bool IgnoreTrailingWhitespace = true,
    bool IgnoreAllWhitespace = false,
    bool IgnoreCase = false)
{
    public static readonly DiffOptions Default = new();
}
