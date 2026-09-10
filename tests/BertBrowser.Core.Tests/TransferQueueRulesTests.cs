using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What a queue of long writes allows. The rule everything follows from is that the running job is
/// first and stays first: it has already written bytes, so moving it would mean either stopping it
/// or lying about the order.
/// </summary>
public class TransferQueueRulesTests
{
    private static QueuedJobSummary Job(
        int id, QueuedJobState state, long bytes = 1024, bool complete = true) =>
        new(id, "Copy", $"job {id}", 1, new TransferEstimate(bytes, 1, complete), state);

    private static IReadOnlyList<QueuedJobSummary> Queue(params QueuedJobSummary[] jobs) => jobs;

    private static int[] Ids(IReadOnlyList<QueuedJobSummary> queue) => [.. queue.Select(j => j.Id)];

    // --- What may move ---

    [Fact]
    public void TheRunningJobCannotBeMoved()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Running),
            Job(2, QueuedJobState.Waiting));

        Assert.False(TransferQueueRules.CanMoveUp(queue, 1));
        Assert.False(TransferQueueRules.CanMoveDown(queue, 1));
    }

    /// <summary>The first waiting job is already as early as anything can be: behind the one that
    /// is running.</summary>
    [Fact]
    public void AWaitingJobCannotOvertakeTheRunningOne()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Running),
            Job(2, QueuedJobState.Waiting),
            Job(3, QueuedJobState.Waiting));

        Assert.False(TransferQueueRules.CanMoveUp(queue, 2));
        Assert.Equal([1, 2, 3], Ids(TransferQueueRules.Move(queue, 2, -1)));
    }

    [Fact]
    public void TheLastWaitingJobCannotMoveDown()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Running),
            Job(2, QueuedJobState.Waiting),
            Job(3, QueuedJobState.Waiting));

        Assert.False(TransferQueueRules.CanMoveDown(queue, 3));
        Assert.Equal([1, 2, 3], Ids(TransferQueueRules.Move(queue, 3, 1)));
    }

    [Fact]
    public void AWaitingJobSwapsWithTheWaitingJobBeforeIt()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Running),
            Job(2, QueuedJobState.Waiting),
            Job(3, QueuedJobState.Waiting),
            Job(4, QueuedJobState.Waiting));

        Assert.True(TransferQueueRules.CanMoveUp(queue, 4));
        Assert.Equal([1, 2, 4, 3], Ids(TransferQueueRules.Move(queue, 4, -1)));
        Assert.Equal([1, 3, 2, 4], Ids(TransferQueueRules.Move(queue, 2, 1)));
    }

    /// <summary>
    /// Finished jobs keep their slot. They are history, and history that reshuffles when the queue
    /// below it is reordered would be a puzzle rather than a record.
    /// </summary>
    [Fact]
    public void FinishedJobsStayWhereTheyAre()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Done),
            Job(2, QueuedJobState.Cancelled),
            Job(3, QueuedJobState.Running),
            Job(4, QueuedJobState.Waiting),
            Job(5, QueuedJobState.Waiting));

        Assert.Equal([1, 2, 3, 5, 4], Ids(TransferQueueRules.Move(queue, 5, -1)));
    }

    [Fact]
    public void MovingNowhere_ChangesNothing()
    {
        var queue = Queue(Job(1, QueuedJobState.Running), Job(2, QueuedJobState.Waiting));

        Assert.Same(queue, TransferQueueRules.Move(queue, 2, 0));
    }

    [Fact]
    public void MovingAJobTheQueueDoesNotHold_ChangesNothing()
    {
        var queue = Queue(Job(1, QueuedJobState.Running), Job(2, QueuedJobState.Waiting));

        Assert.Equal([1, 2], Ids(TransferQueueRules.Move(queue, 99, -1)));
    }

    // --- What is left to write ---

    [Fact]
    public void RemainingCountsTheRunningJobAndEveryWaitingOne()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Done, bytes: 900),
            Job(2, QueuedJobState.Running, bytes: 100),
            Job(3, QueuedJobState.Waiting, bytes: 20),
            Job(4, QueuedJobState.Cancelled, bytes: 700));

        var remaining = TransferQueueRules.Remaining(queue);

        Assert.Equal(120, remaining.Bytes);
        Assert.Equal(2, remaining.Files);
        Assert.True(remaining.IsUsable);
    }

    /// <summary>
    /// One unmeasured job makes the whole total a floor. A percentage drawn over a number nobody
    /// knows is worse than no percentage — the same rule <see cref="TransferEstimate"/> already
    /// applies within one plan.
    /// </summary>
    [Fact]
    public void OneIncompleteEstimate_MakesTheWholeQueueIncomplete()
    {
        var queue = Queue(
            Job(1, QueuedJobState.Running, bytes: 100),
            Job(2, QueuedJobState.Waiting, bytes: 50, complete: false));

        var remaining = TransferQueueRules.Remaining(queue);

        Assert.Equal(150, remaining.Bytes);
        Assert.False(remaining.Complete);
        Assert.False(remaining.IsUsable);
    }

    [Fact]
    public void AnEmptyQueueOwesNothing_AndKnowsIt()
    {
        var remaining = TransferQueueRules.Remaining([]);

        Assert.Equal(0, remaining.Bytes);
        Assert.True(remaining.Complete);
    }

    // --- How it says so ---

    [Fact]
    public void WaitingTextCountsOnlyTheJobsStillToStart()
    {
        Assert.Equal("", TransferQueueRules.WaitingText([]));
        Assert.Equal("", TransferQueueRules.WaitingText(Queue(Job(1, QueuedJobState.Running))));
        Assert.Equal("1 waiting", TransferQueueRules.WaitingText(
            Queue(Job(1, QueuedJobState.Running), Job(2, QueuedJobState.Waiting))));
        Assert.Equal("2 waiting", TransferQueueRules.WaitingText(
            Queue(Job(1, QueuedJobState.Done), Job(2, QueuedJobState.Waiting), Job(3, QueuedJobState.Waiting))));
    }

    /// <summary>A queue of one reads exactly as it did before there was a queue.</summary>
    [Fact]
    public void HeadlineLeavesARunningJobsOwnWordsAlone()
    {
        var queue = Queue(Job(1, QueuedJobState.Running));

        Assert.Equal(
            "Copying 2 of 9 — photo.jpg",
            TransferQueueRules.Headline(queue, "Copying 2 of 9 — photo.jpg", paused: false));
    }

    [Fact]
    public void HeadlineSaysSoWhilePaused()
    {
        var queue = Queue(Job(1, QueuedJobState.Running));

        Assert.Equal(
            "Paused — Copying 2 of 9 — photo.jpg",
            TransferQueueRules.Headline(queue, "Copying 2 of 9 — photo.jpg", paused: true));
        Assert.Equal("Paused", TransferQueueRules.Headline(queue, "", paused: true));
    }
}
