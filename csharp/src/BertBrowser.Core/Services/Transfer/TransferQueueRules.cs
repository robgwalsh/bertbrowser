namespace BertBrowser.Core.Services.Transfer;

/// <summary>Where one queued write has got to.</summary>
public enum QueuedJobState
{
    /// <summary>In the queue, not started. The only state that can be reordered.</summary>
    Waiting,

    /// <summary>Running now. There is at most one of these, and it is always first.</summary>
    Running,

    /// <summary>Finished, however it turned out — the outcome carries the detail.</summary>
    Done,

    /// <summary>Taken out of the queue before it started, or stopped part-way.</summary>
    Cancelled,
}

/// <summary>
/// One queued write, as the queue rules see it: enough to order it and to total it, and nothing
/// about how it is carried out.
/// </summary>
/// <param name="Id">Stable for the job's whole life; the queue is reordered, so an index is not.</param>
/// <param name="Kind">"Copy", "Move", "Extract", "Compress", "Archive edit" — the noun in the row.</param>
/// <param name="Description">"12 items to Archive", already phrased for display.</param>
/// <param name="Items">How many things this job will write.</param>
/// <param name="Estimate">Its byte total, and whether that total can be trusted.</param>
public sealed record QueuedJobSummary(
    int Id,
    string Kind,
    string Description,
    int Items,
    TransferEstimate Estimate,
    QueuedJobState State);

/// <summary>
/// What a queue of long writes allows: what may be reordered, what the whole queue still costs, and
/// how it describes itself.
/// </summary>
/// <remarks>
/// Pure, so the ordering is settled by tests rather than by clicking. The one rule everything else
/// follows from is that <b>the running job is first and stays first</b> — it has already written
/// bytes, so moving it would mean either stopping it or lying about the order.
/// </remarks>
public static class TransferQueueRules
{
    /// <summary>Jobs still to come, in the order they will run. Excludes the running one.</summary>
    public static IReadOnlyList<QueuedJobSummary> Waiting(IReadOnlyList<QueuedJobSummary> queue) =>
        [.. queue.Where(j => j.State == QueuedJobState.Waiting)];

    /// <summary>
    /// True when this job can move one place earlier: it must be waiting, and there must be another
    /// waiting job ahead of it. A waiting job can never overtake the running one.
    /// </summary>
    public static bool CanMoveUp(IReadOnlyList<QueuedJobSummary> queue, int id)
    {
        var waiting = Waiting(queue);
        var at = IndexOf(waiting, id);
        return at > 0;
    }

    /// <summary>True when this job can move one place later.</summary>
    public static bool CanMoveDown(IReadOnlyList<QueuedJobSummary> queue, int id)
    {
        var waiting = Waiting(queue);
        var at = IndexOf(waiting, id);
        return at >= 0 && at < waiting.Count - 1;
    }

    /// <summary>
    /// The queue with one waiting job moved by <paramref name="delta"/> places among the other
    /// waiting ones. A move that is not allowed returns the queue unchanged rather than throwing:
    /// this answers a button, and a button at the end of a list is pressed.
    /// </summary>
    public static IReadOnlyList<QueuedJobSummary> Move(
        IReadOnlyList<QueuedJobSummary> queue, int id, int delta)
    {
        if (delta == 0) return queue;
        if (delta < 0 && !CanMoveUp(queue, id)) return queue;
        if (delta > 0 && !CanMoveDown(queue, id)) return queue;

        // Reorder the waiting jobs among themselves, then lay them back into the slots the waiting
        // jobs occupied. Anything running or finished keeps its place exactly.
        var waiting = Waiting(queue).ToList();
        var at = IndexOf(waiting, id);
        var to = Math.Clamp(at + delta, 0, waiting.Count - 1);
        var moved = waiting[at];
        waiting.RemoveAt(at);
        waiting.Insert(to, moved);

        var next = 0;
        return [.. queue.Select(j => j.State == QueuedJobState.Waiting ? waiting[next++] : j)];
    }

    /// <summary>
    /// What the whole queue still has to write — the running job included, since its own surface
    /// reports how far into it we are.
    /// </summary>
    /// <remarks>
    /// <b>One floor makes the total a floor.</b> If any job's estimate came back incomplete — an
    /// unindexed volume, a folder with no <c>dir_size_cache</c> row — the sum is a lower bound, and
    /// <see cref="TransferEstimate.IsUsable"/> going false is what stops a percentage and a time
    /// remaining being drawn over a number nobody knows.
    /// </remarks>
    public static TransferEstimate Remaining(IReadOnlyList<QueuedJobSummary> queue)
    {
        long bytes = 0;
        var files = 0;
        var complete = true;

        foreach (var job in queue)
        {
            if (job.State is not (QueuedJobState.Waiting or QueuedJobState.Running)) continue;
            bytes += job.Estimate.Bytes;
            files += job.Estimate.Files;
            complete &= job.Estimate.Complete;
        }

        return new TransferEstimate(bytes, files, complete);
    }

    /// <summary>
    /// What the status strip says. The running job's own headline is the subject; the queue only
    /// ever adds to it, so a queue of one reads exactly as it did before there was a queue.
    /// </summary>
    public static string Headline(
        IReadOnlyList<QueuedJobSummary> queue, string runningHeadline, bool paused)
    {
        var text = runningHeadline;
        if (paused) text = text.Length > 0 ? $"Paused — {text}" : "Paused";
        return text;
    }

    /// <summary>
    /// "3 waiting", or nothing at all when the queue holds only what is running. Empty rather than
    /// "0 waiting", so the strip carries no separator with nothing after it.
    /// </summary>
    public static string WaitingText(IReadOnlyList<QueuedJobSummary> queue) =>
        Waiting(queue).Count switch
        {
            0 => "",
            var n => $"{n:N0} waiting",
        };

    private static int IndexOf(IReadOnlyList<QueuedJobSummary> jobs, int id)
    {
        for (var i = 0; i < jobs.Count; i++)
            if (jobs[i].Id == id) return i;
        return -1;
    }
}
