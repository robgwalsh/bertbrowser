namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>Where a walk through the history ended, and what it leaves for the caller to do.</summary>
/// <param name="StepsTaken">Steps that moved something, partial ones included.</param>
/// <param name="StoppedAt">The entry whose step was not clean, when one was.</param>
/// <param name="StopReport">That step's report.</param>
/// <param name="Refreshes">What every step taken leaves stale on screen.</param>
/// <param name="Released">What the walk pushed out of the budget, for the caller to retire.</param>
public sealed record NavigationResult(
    int StepsTaken,
    UndoEntry? StoppedAt,
    StepReport? StopReport,
    IReadOnlyList<UndoRefresh> Refreshes,
    Released Released,
    IReadOnlyList<string> Notes);

/// <summary>
/// Walks the history to a chosen entry — "undo to here", "redo to here" — one step at a time.
/// </summary>
/// <remarks>
/// <b>It stops at the first step that is not clean.</b> Each entry's operation may depend on the one
/// before it being undone completely — a rename undone after a move that only half went back would
/// rename whatever is standing at those names now. So a partial step is taken, reported, and the
/// walk goes no further: "undo 5" that trips on the third leaves the fourth and fifth done.
/// </remarks>
public static class UndoNavigator
{
    /// <param name="measure">How much a record holds, called on the thread pool.</param>
    public static async Task<NavigationResult> WalkAsync(
        UndoStack stack,
        IReadOnlyList<UndoEntry> span,
        IUndoHost host,
        Func<IUndoableRecord, HeldSize> measure,
        Func<DateTime> clock)
    {
        var refreshes = new List<UndoRefresh>();
        var released = new List<UndoEntry>();
        var notes = new List<string>();
        var steps = 0;

        foreach (var entry in span)
        {
            var result = await entry.Record.StepAsync(host);
            refreshes.Add(result.Refresh);
            if (result.Report.Note.Length > 0) notes.Add(result.Report.Note);

            if (result.Report.NothingHappened)
            {
                stack.Refused(entry, result.Report, clock());
                return new NavigationResult(steps, entry, result.Report, refreshes, new Released(released), notes);
            }

            var held = await Task.Run(() => measure(result.Next));
            released.AddRange(stack.Applied(entry, result.Next, held, result.Report, clock()).Entries);
            steps++;

            if (!result.Report.Clean)
                return new NavigationResult(steps, entry, result.Report, refreshes, new Released(released), notes);
        }

        return new NavigationResult(steps, null, null, refreshes, new Released(released), notes);
    }
}
