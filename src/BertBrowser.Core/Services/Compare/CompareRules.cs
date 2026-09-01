namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// What a verdict means to the rest of the app: what a row shows, what the filter keeps, and what
/// a sync would do about it. Pure, so every one of these answers is pinned by a test rather than
/// spread across view models.
/// </summary>
public static class CompareRules
{
    /// <summary>
    /// How one side renders a verdict. The mirror is the point: a left-newer pair is "newer" on the
    /// left and "older" on the right, and a left-only pair has nothing at all to say on the right,
    /// because there is no row there to colour.
    /// </summary>
    public static CompareRowState RowState(CompareVerdict verdict, CompareSide side) => verdict switch
    {
        CompareVerdict.Unknown => CompareRowState.Unknown,
        CompareVerdict.Same => CompareRowState.Same,
        CompareVerdict.Differs => CompareRowState.Differs,
        CompareVerdict.LeftOnly => side is CompareSide.Left ? CompareRowState.OnlyHere : CompareRowState.None,
        CompareVerdict.RightOnly => side is CompareSide.Right ? CompareRowState.OnlyHere : CompareRowState.None,
        CompareVerdict.LeftNewer => side is CompareSide.Left ? CompareRowState.Newer : CompareRowState.Older,
        CompareVerdict.RightNewer => side is CompareSide.Right ? CompareRowState.Newer : CompareRowState.Older,
        _ => CompareRowState.None,
    };

    /// <summary>
    /// What "show only differences" keeps.
    /// </summary>
    /// <remarks>
    /// <b><see cref="CompareVerdict.Unknown"/> counts as a difference.</b> It is the absence of a
    /// match, not a match — and hiding it would be the one way a row that could not be compared
    /// disappears from the screen the user is about to sync from.
    /// </remarks>
    public static bool IsDifference(CompareVerdict verdict) => verdict is not CompareVerdict.Same;

    /// <summary>Verdicts a left-to-right sync answers by writing to the right.</summary>
    /// <remarks><see cref="CompareVerdict.RightNewer"/> is included because "make right match left"
    /// is not finished while the right side still holds a different file — but the planner leaves
    /// those actions unticked by default (see <see cref="OverwritesNewer"/>), because overwriting
    /// the newer of two files is the one copy a user would not expect to have agreed to.</remarks>
    public static bool WouldCopy(CompareVerdict verdict) =>
        verdict is CompareVerdict.LeftOnly or CompareVerdict.LeftNewer
                or CompareVerdict.RightNewer or CompareVerdict.Differs;

    /// <summary>A copy that would replace a file the right side updated more recently.</summary>
    public static bool OverwritesNewer(CompareVerdict verdict) => verdict is CompareVerdict.RightNewer;

    /// <summary>Verdicts a left-to-right sync answers by removing from the right.</summary>
    public static bool WouldDelete(CompareVerdict verdict) => verdict is CompareVerdict.RightOnly;

    /// <summary>
    /// Folds one child's verdict into the folder above it.
    /// </summary>
    /// <remarks>
    /// <para>
    /// A child never tells a folder <em>which</em> side it is missing from — only that something
    /// beneath it is not settled. So a child contributes one of three signals: unknown, different,
    /// or nothing. That is what stops a folder that exists on both sides being called "left only"
    /// because one file inside it is.
    /// </para>
    /// <para>
    /// The ranking is <c>Same &lt; Differs &lt; LeftOnly/RightOnly &lt; Unknown</c>.
    /// <see cref="CompareVerdict.Unknown"/> outranking everything is deliberate: "differs" invites
    /// a copy, which is additive and safe, while "same" invites a delete. The only verdict that
    /// must never be arrived at by not knowing is <see cref="CompareVerdict.Same"/>.
    /// </para>
    /// </remarks>
    public static CompareVerdict RollUp(CompareVerdict folder, CompareVerdict child)
    {
        var signal = child switch
        {
            CompareVerdict.Unknown => CompareVerdict.Unknown,
            CompareVerdict.Same => CompareVerdict.Same,
            _ => CompareVerdict.Differs,
        };

        return Rank(signal) > Rank(folder) ? signal : folder;
    }

    private static int Rank(CompareVerdict verdict) => verdict switch
    {
        CompareVerdict.Same => 0,
        CompareVerdict.Differs or CompareVerdict.LeftNewer or CompareVerdict.RightNewer => 1,
        CompareVerdict.LeftOnly or CompareVerdict.RightOnly => 2,
        _ => 3,
    };

    /// <summary>One side's phrasing of a verdict, for a tooltip, a status column and the summary.</summary>
    public static string Describe(CompareVerdict verdict, CompareSide side) =>
        RowState(verdict, side) switch
        {
            CompareRowState.Unknown => "Could not be compared",
            CompareRowState.Same => "Same",
            CompareRowState.OnlyHere => side is CompareSide.Left ? "Only on the left" : "Only on the right",
            CompareRowState.Newer => "Newer",
            CompareRowState.Older => "Older",
            CompareRowState.Differs => "Differs",
            _ => "",
        };
}
