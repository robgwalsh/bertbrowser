using BertBrowser.Core.Services.UndoHistory;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The history's rules, with records that touch nothing: what a new operation discards, what the
/// budget releases and in which order, and what one step may and may not change.
/// </summary>
public sealed class UndoStackTests
{
    private static readonly DateTime Now = new(2026, 9, 25, 12, 0, 0, DateTimeKind.Utc);

    private static UndoStack Stack(int entries = 50, long bytes = 1L << 40) => new(new UndoBudget(entries, bytes));

    private static (UndoEntry Entry, Released Released) Push(UndoStack stack, string name, long held = 0) =>
        stack.Push(new FakeRecord(name), new HeldSize(held, true), Now);

    /// <summary>Takes the step the stack says is next on <paramref name="entry"/>, cleanly.</summary>
    private static Released Step(UndoStack stack, UndoEntry entry, long held = 0) =>
        stack.Applied(entry, ((FakeRecord)entry.Record).Flipped(), new HeldSize(held, true),
            new StepReport(1, []), Now);

    private static string[] Names(UndoStack stack) =>
        [.. stack.Entries.Select(e => e.Record.Description)];

    [Fact]
    public void ANewOperationIsDoneAndIsTheNextUndo()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");

        Assert.True(a.IsDone);
        Assert.Same(a, stack.NextUndo);
        Assert.Null(stack.NextRedo);
        Assert.Equal(1, stack.Cursor);
    }

    [Fact]
    public void UndoingMovesTheCursorAndMakesTheEntryTheNextRedo()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");
        var (b, _) = Push(stack, "b");

        Step(stack, b);

        Assert.False(b.IsDone);
        Assert.Same(a, stack.NextUndo);
        Assert.Same(b, stack.NextRedo);
        Assert.Equal(UndoDirection.Undo, b.LastDirection);
    }

    /// <summary>One line of history, as in every editor: a new operation after an undo abandons
    /// the branch, and hands it back to be retired so what it held is committed.</summary>
    [Fact]
    public void ANewOperationDiscardsTheRedoBranchAndReleasesIt()
    {
        var stack = Stack();
        Push(stack, "a");
        var (b, _) = Push(stack, "b");
        var (c, _) = Push(stack, "c");
        Step(stack, c);
        Step(stack, b);

        var (_, released) = Push(stack, "d");

        Assert.Equal(["a", "d"], Names(stack));
        Assert.Equal(["b", "c"], released.Entries.Select(e => e.Record.Description).Order());
        Assert.Equal(0, stack.ReleasedCount); // abandoned, not pushed out by the limits
    }

    [Fact]
    public void TheCountCapReleasesTheOldestFirst()
    {
        var stack = Stack(entries: 3);
        Push(stack, "a");
        Push(stack, "b");
        Push(stack, "c");

        var (_, released) = Push(stack, "d");

        Assert.Equal(["b", "c", "d"], Names(stack));
        Assert.Equal(["a"], released.Entries.Select(e => e.Record.Description));
        Assert.Equal(1, stack.ReleasedCount);
    }

    [Fact]
    public void TheByteCapReleasesTheOldestDoneEntryFirst()
    {
        var stack = Stack(bytes: 100);
        Push(stack, "a", held: 40);
        Push(stack, "b", held: 40);

        var (_, released) = Push(stack, "c", held: 40);

        Assert.Equal(["b", "c"], Names(stack));
        Assert.Equal(40, stack.ReleasedBytes);
        Assert.Equal(40, released.Held.Bytes);
    }

    /// <summary>
    /// With nothing old enough left to release, the far end of the redo branch goes next.
    /// </summary>
    [Fact]
    public void TheByteCapThenReleasesTheFurthestUndoneEntry()
    {
        var stack = Stack(bytes: 100);
        var (a, _) = Push(stack, "a", held: 10);
        var (b, _) = Push(stack, "b", held: 10);
        var (c, _) = Push(stack, "c", held: 10);
        Step(stack, c, held: 10);
        Step(stack, b, held: 10);

        // Undoing a grows what it holds — an archive's edited copy, say — past the budget.
        var released = Step(stack, a, held: 85);

        Assert.Equal(["a", "b"], Names(stack));
        Assert.Equal(["c"], released.Entries.Select(e => e.Record.Description));
    }

    /// <summary>
    /// A single operation larger than the whole budget stays undoable — the guarantee the single
    /// slot always gave.
    /// </summary>
    [Fact]
    public void TheNextUndoIsNeverReleasedEvenOverBudget()
    {
        var stack = Stack(entries: 1, bytes: 10);
        Push(stack, "a", held: 5);

        var (b, released) = Push(stack, "b", held: 1_000);

        Assert.Equal(["b"], Names(stack));
        Assert.Same(b, stack.NextUndo);
        Assert.Equal(["a"], released.Entries.Select(e => e.Record.Description));
    }

    [Fact]
    public void ShrinkingTheBudgetReleasesStraightAway()
    {
        var stack = Stack();
        Push(stack, "a");
        Push(stack, "b");
        Push(stack, "c");

        var released = stack.SetBudget(new UndoBudget(1, 1L << 40));

        Assert.Equal(["c"], Names(stack));
        Assert.Equal(2, released.Entries.Count);
    }

    /// <summary>A size that could only partly be measured counts as what was measured — a floor —
    /// never as nothing.</summary>
    [Fact]
    public void AnIncompleteSizeStillCountsAgainstTheBudget()
    {
        var stack = Stack(bytes: 100);
        stack.Push(new FakeRecord("a"), new HeldSize(80, Complete: false), Now);
        stack.Push(new FakeRecord("b"), new HeldSize(80, Complete: false), Now);

        Assert.Equal(["b"], Names(stack));
        Assert.False(stack.Held.Complete);
    }

    [Fact]
    public void UndoSpanRunsFromTheNextUndoBackToTheTarget()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");
        Push(stack, "b");
        Push(stack, "c");

        Assert.Equal(["c", "b", "a"], stack.UndoSpan(a).Select(e => e.Record.Description));
    }

    [Fact]
    public void RedoSpanRunsFromTheNextRedoForwardToTheTarget()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");
        var (b, _) = Push(stack, "b");
        var (c, _) = Push(stack, "c");
        Step(stack, c);
        Step(stack, b);
        Step(stack, a);

        Assert.Equal(["a", "b"], stack.RedoSpan(b).Select(e => e.Record.Description));
    }

    [Fact]
    public void ASpanOnTheWrongSideOfTheCursorIsRefused()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");

        Assert.Throws<ArgumentException>(() => stack.RedoSpan(a));
    }

    /// <summary>Only the entry either side of the cursor can take a step: anything else would undo
    /// an operation out from under the ones built on it.</summary>
    [Fact]
    public void OnlyTheEntriesBesideTheCursorCanStep()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");
        Push(stack, "b");

        Assert.Throws<ArgumentException>(() => Step(stack, a));
    }

    [Fact]
    public void ARefusedStepLeavesTheEntryWhereItWas()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");

        stack.Refused(a, new StepReport(0, ["nope"]), Now);

        Assert.True(a.IsDone);
        Assert.Equal(1, stack.Cursor);
        Assert.Equal(StepVerdict.Failed, a.LastReport!.Verdict);
    }

    [Fact]
    public void APartialStepStillMovesTheCursor()
    {
        var stack = Stack();
        var (a, _) = Push(stack, "a");

        stack.Applied(a, new FakeRecord("a", applied: false), HeldSize.None, new StepReport(2, ["one stuck"]), Now);

        Assert.False(a.IsDone);
        Assert.Equal(StepVerdict.Partial, a.LastReport!.Verdict);
    }

    [Fact]
    public void ClearingReleasesEverythingWithoutCountingItAgainstTheLimits()
    {
        var stack = Stack();
        Push(stack, "a", held: 3);
        var (b, _) = Push(stack, "b", held: 4);
        Step(stack, b);

        var released = stack.Clear();

        Assert.Empty(stack.Entries);
        Assert.Null(stack.NextUndo);
        Assert.Equal(2, released.Entries.Count);
        Assert.Equal(0, stack.ReleasedCount);
    }

    [Fact]
    public void RetiringAReleaseRetiresEveryRecordInIt()
    {
        var stack = Stack(entries: 1);
        var (a, _) = Push(stack, "a");
        var (_, released) = Push(stack, "b");

        released.RetireAll();

        Assert.True(((FakeRecord)a.Record).Retired);
    }
}

/// <summary>A record that touches nothing and does exactly what it is scripted to.</summary>
internal sealed class FakeRecord(string name, bool applied = true, StepReport? report = null) : IUndoableRecord
{
    public bool Retired { get; private set; }

    public int Steps { get; private set; }

    public UndoKind Kind => UndoKind.Rename;

    public bool IsApplied => applied;

    public string Description => name;

    public int ItemCount => 1;

    public IReadOnlyList<UndoItemDetail> Details => [];

    public IReadOnlyList<HeldEntry> Held => [];

    /// <summary>What the next step will report; clean by default.</summary>
    public StepReport? Report { get; set; } = report;

    public FakeRecord Flipped() => new(name, !applied) { Report = Report };

    public Task<StepResult> StepAsync(IUndoHost host)
    {
        Steps++;
        var report = Report ?? new StepReport(1, []);
        IUndoableRecord next = report.NothingHappened ? this : Flipped();
        return Task.FromResult(new StepResult(next, report, UndoRefresh.Of([name])));
    }

    public void Retire() => Retired = true;
}
