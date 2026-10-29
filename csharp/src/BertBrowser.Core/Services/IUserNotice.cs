namespace BertBrowser.Core.Services;

/// <summary>
/// Telling the user something they have to acknowledge, without knowing what a window is.
/// </summary>
/// <remarks>
/// <para>
/// For the answer to a gesture they just made that cannot be carried out — "comparing needs two
/// panes" and nothing else so far. Not for progress, not for what an operation did afterwards, and
/// not for anything a status line can carry: those belong in the status bar, which does not
/// interrupt.
/// </para>
/// <para>
/// An injected service rather than an event the window handles, for the reason
/// <see cref="Elevation.IElevationPrompt"/> is one: a modal raised from inside the shell would
/// block the scripted run that hosts the same window offscreen, so a run needs to be able to hand
/// it something that records instead of showing.
/// </para>
/// </remarks>
public interface IUserNotice
{
    void Say(string message, string caption);
}

/// <summary>Says nothing. What a context with nobody to tell gets.</summary>
public sealed class SilentUserNotice : IUserNotice
{
    public void Say(string message, string caption)
    {
    }
}
