using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The latch a long write waits on. Its two unusual guarantees — that it never throws, and that a
/// cancelled wait says so rather than pretending it was resumed — exist because it is waited on
/// from inside a native <c>CopyFileExW</c> callback.
/// </summary>
public class PauseGateTests
{
    /// <summary>How long something that should happen is given before it is called a hang.</summary>
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(5);

    /// <summary>Long enough that a run which was going to proceed would have.</summary>
    private static readonly TimeSpan LongEnoughToNotice = TimeSpan.FromMilliseconds(200);

    private static async Task<bool> Finished(Task task, TimeSpan within) =>
        await Task.WhenAny(task, Task.Delay(within)) == task;

    [Fact]
    public void AGateStartsOpen_AndWaitingOnItReturnsAtOnce()
    {
        using var gate = new PauseGate();

        Assert.False(gate.IsPaused);
        Assert.True(gate.Wait(CancellationToken.None));
    }

    [Fact]
    public async Task Pausing_HoldsAWaiter_UntilItIsResumed()
    {
        using var gate = new PauseGate();
        using var entered = new ManualResetEventSlim(false);
        gate.Pause();

        var result = false;
        var waiter = Task.Run(() =>
        {
            entered.Set();
            result = gate.Wait(CancellationToken.None);
        });

        Assert.True(entered.Wait(Patience));
        Assert.False(await Finished(waiter, LongEnoughToNotice));
        Assert.True(gate.IsPaused);

        gate.Resume();

        await waiter.WaitAsync(Patience);
        Assert.True(result);
        Assert.False(gate.IsPaused);
    }

    /// <summary>
    /// The guarantee the copier depends on: a cancel while paused unblocks, and reports itself as a
    /// cancel rather than as a resume — and does it without an exception, which would unwind across
    /// the P/Invoke boundary out of a callback Windows is still inside.
    /// </summary>
    [Fact]
    public async Task CancellingAPausedWait_ReturnsFalse_WithoutThrowing()
    {
        using var gate = new PauseGate();
        using var cancellation = new CancellationTokenSource();
        using var entered = new ManualResetEventSlim(false);
        gate.Pause();

        var result = true;
        var waiter = Task.Run(() =>
        {
            entered.Set();
            result = gate.Wait(cancellation.Token);
        });

        Assert.True(entered.Wait(Patience));
        await cancellation.CancelAsync();

        await waiter.WaitAsync(Patience);
        Assert.False(result);
        Assert.True(gate.IsPaused); // cancelling a wait does not open the gate; the caller does
    }

    /// <summary>An already-cancelled run gets the same answer without ever having to block.</summary>
    [Fact]
    public void AnOpenGate_StillReportsAnAlreadyCancelledRun()
    {
        using var gate = new PauseGate();
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        Assert.False(gate.Wait(cancellation.Token));
    }

    [Fact]
    public void PausingTwice_IsStillOneResumeAway()
    {
        using var gate = new PauseGate();
        gate.Pause();
        gate.Pause();
        gate.Resume();

        Assert.False(gate.IsPaused);
        Assert.True(gate.Wait(CancellationToken.None));
    }
}
