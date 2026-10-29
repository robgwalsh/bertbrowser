using BertBrowser.Core.Services.Foreground;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ForegroundRaiseRulesTests
{
    private const bool Ours = true;
    private const bool Theirs = false;
    private const bool Fills = true;
    private const bool Windowed = false;

    [Theory]
    // The bug: a hand-off arriving while something is full screen must not take the screen.
    [InlineData(UserNotificationState.Busy, Theirs, Windowed, RaiseAction.Flash)]
    [InlineData(UserNotificationState.RunningD3DFullScreen, Theirs, Windowed, RaiseAction.Flash)]
    [InlineData(UserNotificationState.PresentationMode, Theirs, Windowed, RaiseAction.Flash)]
    // A Store app only counts when it is actually full screen — otherwise opening a folder from
    // Mail or Settings would merely flash.
    [InlineData(UserNotificationState.App, Theirs, Fills, RaiseAction.Flash)]
    [InlineData(UserNotificationState.App, Theirs, Windowed, RaiseAction.Raise)]
    // The ordinary case, and the two states that are about notifications rather than the screen.
    [InlineData(UserNotificationState.AcceptsNotifications, Theirs, Windowed, RaiseAction.Raise)]
    [InlineData(UserNotificationState.QuietTime, Theirs, Windowed, RaiseAction.Raise)]
    [InlineData(UserNotificationState.NotPresent, Theirs, Windowed, RaiseAction.Raise)]
    // A shell that would not answer must not cost the user the folder they asked for.
    [InlineData(UserNotificationState.Unknown, Theirs, Windowed, RaiseAction.Raise)]
    [InlineData(UserNotificationState.Unknown, Theirs, Fills, RaiseAction.Raise)]
    // Geometry alone is not full screen: a maximized window with the taskbar auto-hidden fills its
    // monitor too, and the state is what separates the two.
    [InlineData(UserNotificationState.AcceptsNotifications, Theirs, Fills, RaiseAction.Raise)]
    public void Decide_WithholdsTheRaiseOnlyForSomethingFullScreen(
        UserNotificationState state, bool ours, bool fills, RaiseAction expected)
    {
        Assert.Equal(expected, ForegroundRaiseRules.Decide(state, ours, fills));
    }

    [Theory]
    [InlineData(UserNotificationState.Busy)]
    [InlineData(UserNotificationState.RunningD3DFullScreen)]
    [InlineData(UserNotificationState.PresentationMode)]
    [InlineData(UserNotificationState.App)]
    public void Decide_RaisesWhenWeAreAlreadyTheForeground(UserNotificationState state)
    {
        // Nothing of anyone else's is being interrupted, and the raise is what makes a hand-off
        // into the session the user is already looking at visible.
        Assert.Equal(RaiseAction.Raise, ForegroundRaiseRules.Decide(state, Ours, Fills));
    }
}
