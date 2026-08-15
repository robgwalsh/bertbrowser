namespace BertBrowser.App.Views;

/// <summary>
/// One drag, for as long as it lasts, and the one fact worth remembering about it: whether this
/// app's own drop pipeline took the drop.
/// </summary>
/// <remarks>
/// <para>
/// This exists to close a real trap. <c>DropPipeline</c> handles an in-app drop and
/// <c>TransferExecutor</c> has already relocated the items — but <c>DoDragDrop</c> still reports
/// <c>Move</c>, and acting on that would delete what we had just placed. Two independent guards
/// stand against it: this flag, and <c>DropPipeline</c> zeroing <c>e.Effects</c>. Either alone is
/// enough, which is the point — a future third drop target that forgets one is still safe.
/// </para>
/// <para>
/// A static is correct here rather than lazy: <c>DoDragDrop</c> is modal and runs on the UI thread,
/// so exactly one drag exists at a time and it belongs to that thread. A foreign process cannot
/// reach this field, which is precisely what makes it a trustworthy answer to "was that one of
/// ours?".
/// </para>
/// </remarks>
internal sealed class DragSession : IDisposable
{
    private static DragSession? _current;

    private DragSession()
    {
    }

    /// <summary>True once one of this app's own drop targets has taken the drop.</summary>
    public bool HandledInApp { get; private set; }

    /// <summary>Starts a drag. Dispose when <c>DoDragDrop</c> returns.</summary>
    public static DragSession Begin() => _current = new DragSession();

    /// <summary>Called by <see cref="DropPipeline"/> the moment it recognises the payload as ours.
    /// A no-op when the drag started in another process, which is exactly right.</summary>
    public static void ClaimInApp()
    {
        if (_current is { } session) session.HandledInApp = true;
    }

    public void Dispose()
    {
        if (ReferenceEquals(_current, this)) _current = null;
    }
}
