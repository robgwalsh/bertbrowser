namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// A latch a long write waits on, so the user can hold it still and let it go again.
/// </summary>
/// <remarks>
/// <para>
/// <b>It is waited on from inside a native callback</b>, which is what makes the two odd parts of
/// this class necessary. <c>CopyFileExW</c> invokes its progress routine on the thread doing the
/// copy, and <see cref="FileSystemFileCopier"/> forwards that straight to the
/// <see cref="Action{T1,T2}"/> the executor passed — so a progress delegate that blocks blocks the
/// OS copy itself, mid-file. That is the whole mechanism: there is no pause verb to send anywhere.
/// </para>
/// <para>
/// <b>Which is also why <see cref="Wait"/> never throws.</b> An <see cref="OperationCanceledException"/>
/// raised here would unwind across the P/Invoke boundary out of a callback Windows is still inside.
/// A cancelled wait returns <c>false</c> instead; the copier's own token check stops the copy at the
/// next chunk, one chunk later than a throw would have — which is invisible, and safe.
/// </para>
/// <para>
/// Cancelling therefore has to <see cref="Resume"/> as well as trip the token. The token alone
/// releases <em>this</em> wait, but leaves the gate shut for whatever runs next.
/// </para>
/// </remarks>
public sealed class PauseGate : IDisposable
{
    /// <summary>Set means open. A gate starts open: nothing is paused until someone asks.</summary>
    private readonly ManualResetEventSlim _open = new(initialState: true);

    public bool IsPaused => !_open.IsSet;

    public void Pause() => _open.Reset();

    public void Resume() => _open.Set();

    /// <summary>
    /// Blocks while paused. Returns false when the wait ended because the run was cancelled rather
    /// than resumed — see the remarks for why this reports rather than throws.
    /// </summary>
    public bool Wait(CancellationToken ct)
    {
        // The overwhelmingly common case, taken without touching the token or the event's slow path.
        if (_open.IsSet) return !ct.IsCancellationRequested;

        try
        {
            _open.Wait(ct);
            return true;
        }
        catch (OperationCanceledException)
        {
            return false;
        }
        catch (ObjectDisposedException)
        {
            // The run this gate belonged to is over; whoever is still in a callback should stop.
            return false;
        }
    }

    public void Dispose() => _open.Dispose();
}
