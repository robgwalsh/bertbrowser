using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// The one place the app asks Windows what it looks like.
/// </summary>
/// <remarks>
/// Behind an interface so the offscreen harness can pose a light, dark or high-contrast machine
/// without touching the real one — and so a screenshot does not depend on how the developer running
/// it has their own Windows set up, which the direct <c>SystemParameters.HighContrast</c> read this
/// replaces already got wrong.
/// </remarks>
public interface ISystemAppearance : IDisposable
{
    /// <summary>Read on demand, on the UI thread.</summary>
    SystemAppearance Current { get; }

    /// <summary>Raised on the dispatcher thread, and only when <see cref="Current"/> actually
    /// changed value — Windows sends its notification for a great many things we do not care
    /// about.</summary>
    event EventHandler? Changed;
}
