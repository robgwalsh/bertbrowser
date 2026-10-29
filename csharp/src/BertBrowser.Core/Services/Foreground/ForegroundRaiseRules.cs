namespace BertBrowser.Core.Services.Foreground;

/// <summary>
/// The shell's answer to "is now a good moment to interrupt the user?" — the values
/// <c>SHQueryUserNotificationState</c> returns, plus <see cref="Unknown"/> for a call that failed.
/// </summary>
public enum UserNotificationState
{
    /// <summary>The query failed or returned something this build does not know. Treated as
    /// permissive: a raise the user asked for must not be lost to a shell that would not answer.</summary>
    Unknown = 0,
    NotPresent = 1,
    /// <summary>A full-screen application is running, or presentation settings are applied.</summary>
    Busy = 2,
    /// <summary>A full-screen Direct3D application has the display.</summary>
    RunningD3DFullScreen = 3,
    PresentationMode = 4,
    AcceptsNotifications = 5,
    QuietTime = 6,
    /// <summary>A Windows Store app is running. Says nothing about full screen on its own — which
    /// is why <see cref="ForegroundRaiseRules"/> asks for the geometry too.</summary>
    App = 7,
}

/// <summary>What to do with a window a hand-off has just filled with the folder someone asked for.</summary>
public enum RaiseAction
{
    /// <summary>Restore it and take the foreground: the request is worthless in a window behind
    /// whatever the user was looking at.</summary>
    Raise,

    /// <summary>Leave it exactly where it is — minimised if it was minimised — and flash its taskbar
    /// button instead.</summary>
    Flash,
}

/// <summary>
/// Whether the single-instance hand-off may pull the window to the front.
/// </summary>
/// <remarks>
/// <para>
/// <b>Never steal the foreground from a full-screen app.</b> <c>ForegroundWindow.Raise</c> is built
/// to defeat the foreground lock on purpose, and the app is the registered handler for
/// <c>Directory</c> and <c>Drive</c> — so the hand-off does not only fire when the user
/// double-clicks a folder. Anything on the system that shell-opens one (a notification's "open
/// folder", an installer, a sync client, another app's "show in folder") starts a second copy, which
/// grants its foreground rights over, and the running copy then lands on top of a full-screen video
/// or game. That is the bug this exists to stop, and it has no pattern from the user's side because
/// the trigger belongs to some other process.
/// </para>
/// <para>
/// The request itself is still honoured either way — the tab opens; only the raise is withheld, and
/// a flashing taskbar button says so. Withholding costs one click; taking it costs whatever was
/// full screen.
/// </para>
/// <para>Pure, so the table below is tests rather than something to catch by playing a video.</para>
/// </remarks>
public static class ForegroundRaiseRules
{
    /// <param name="state">What <c>SHQueryUserNotificationState</c> said, or
    /// <see cref="UserNotificationState.Unknown"/> if it could not be asked.</param>
    /// <param name="foregroundIsOurs">Whether the foreground window already belongs to this
    /// process. Then there is nothing to interrupt — a second window of our own, or the very window
    /// being raised — and the raise is what makes a hand-off into an existing session visible.</param>
    /// <param name="foregroundCoversMonitor">Whether the foreground window's rectangle covers its
    /// whole monitor, frame and all. Only consulted for a Store app, which is what a modern
    /// full-screen player is and which <see cref="UserNotificationState.App"/> alone cannot
    /// distinguish from one in a window.</param>
    public static RaiseAction Decide(
        UserNotificationState state, bool foregroundIsOurs, bool foregroundCoversMonitor)
    {
        if (foregroundIsOurs) return RaiseAction.Raise;

        return state switch
        {
            // Unambiguous: the shell is already telling every other app to stay quiet.
            UserNotificationState.Busy or
            UserNotificationState.RunningD3DFullScreen or
            UserNotificationState.PresentationMode => RaiseAction.Flash,

            // A Store app is only in the way when it is actually full screen. Suppressing on the
            // state alone would mean a folder opened from Mail or Settings merely flashed.
            UserNotificationState.App when foregroundCoversMonitor => RaiseAction.Flash,

            _ => RaiseAction.Raise,
        };
    }
}
