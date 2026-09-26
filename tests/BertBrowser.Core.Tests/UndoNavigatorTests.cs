using BertBrowser.Core.Services.UndoHistory;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// "Undo to here" and "redo to here": a walk of single steps that stops at the first one that did
/// not go cleanly, so nothing runs on top of an operation that is only half undone.
/// </summary>
public sealed class UndoNavigatorTests
{
    private static readonly DateTime Now = new(2026, 9, 25, 12, 0, 0, DateTimeKind.Utc);

    private readonly UndoStack _stack = new(UndoBudget.Default);

    private UndoEntry Push(string name) => _stack.Push(new FakeRecord(name), HeldSize.None, Now).Entry;

    private Task<NavigationResult> Walk(IReadOnlyList<UndoEntry> span) =>
        UndoNavigator.WalkAsync(_stack, span, host: null!, _ => HeldSize.None, () => Now);

    [Fact]
    public async Task UndoingToAnEntryUndoesItAndEverythingAfterIt()
    {
        var a = Push("a");
        var b = Push("b");
        var c = Push("c");

        var result = await Walk(_stack.UndoSpan(a));

        Assert.Equal(3, result.StepsTaken);
        Assert.Null(result.StoppedAt);
        Assert.All([a, b, c], e => Assert.False(e.IsDone));
        Assert.Same(a, _stack.NextRedo);
        Assert.Equal(3, result.Refreshes.Count);
    }

    [Fact]
    public async Task RedoingToAnEntryRedoesUpToItAndNoFurther()
    {
        var a = Push("a");
        var b = Push("b");
        var c = Push("c");
        await Walk(_stack.UndoSpan(a));

        var result = await Walk(_stack.RedoSpan(b));

        Assert.Equal(2, result.StepsTaken);
        Assert.True(a.IsDone);
        Assert.True(b.IsDone);
        Assert.False(c.IsDone);
    }

    /// <summary>A step that half worked is taken and reported, and the walk goes no further.</summary>
    [Fact]
    public async Task APartialStepIsTheLastOne()
    {
        var a = Push("a");
        var b = Push("b");
        var c = Push("c");
        ((FakeRecord)b.Record).Report = new StepReport(1, ["one could not be put back"]);

        var result = await Walk(_stack.UndoSpan(a));

        Assert.Equal(2, result.StepsTaken);
        Assert.Same(b, result.StoppedAt);
        Assert.False(c.IsDone);
        Assert.False(b.IsDone);
        Assert.True(a.IsDone);
        Assert.Equal(0, ((FakeRecord)a.Record).Steps);
    }

    /// <summary>A step that moved nothing leaves its entry where it was, and stops the walk.</summary>
    [Fact]
    public async Task AFailedStepLeavesItsEntryAndStops()
    {
        var a = Push("a");
        var b = Push("b");
        ((FakeRecord)b.Record).Report = new StepReport(0, ["everything was in the way"]);

        var result = await Walk(_stack.UndoSpan(a));

        Assert.Equal(0, result.StepsTaken);
        Assert.Same(b, result.StoppedAt);
        Assert.True(b.IsDone);
        Assert.Same(b, _stack.NextUndo);
        Assert.Equal(StepVerdict.Failed, b.LastReport!.Verdict);
    }
}
