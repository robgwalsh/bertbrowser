using System.IO;
using System.Windows;
using System.Windows.Threading;
using BertBrowser.Core.Theming;
using Microsoft.Win32;

namespace BertBrowser.App.Theming;

/// <summary>
/// Reads the real Windows light/dark and high-contrast settings, and reports when they change.
/// </summary>
/// <remarks>
/// <para>
/// The light/dark bit is <c>AppsUseLightTheme</c> and deliberately not <c>SystemUsesLightTheme</c>,
/// which is the taskbar and Start colour — plenty of people run the two opposite ways, and reading
/// the wrong one gives a feature that is right on most machines and inexplicably backwards on some.
/// A missing value reads as light, which is what Windows itself falls back to.
/// </para>
/// <para>
/// The notification is <see cref="SystemEvents.UserPreferenceChanged"/> rather than a
/// <c>WM_SETTINGCHANGE</c> hook in <see cref="Views.ThemedWindow"/>: that one is per window, and
/// windows come and go, while the theme has to be settled before <c>MainWindow</c> exists at all.
/// No category is filtered on, because which one carries this differs by Windows build — the
/// immersive colour set arrives as <c>General</c>, high contrast as <c>Accessibility</c>, and both
/// as others besides. Re-reading two values and comparing is cheaper than being wrong, and it
/// collapses the burst of notifications Windows sends for one toggle into a single change.
/// </para>
/// </remarks>
public sealed class WindowsSystemAppearance : ISystemAppearance
{
    private const string PersonalizeKey =
        @"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    private readonly Dispatcher _dispatcher;
    private SystemAppearance _last;
    private bool _disposed;

    public WindowsSystemAppearance()
    {
        _dispatcher = Application.Current?.Dispatcher ?? Dispatcher.CurrentDispatcher;
        _last = Read();

        // UserPreferenceChanged is a *static* event, so this handler roots this object, the
        // ThemeService holding it and everything under that for the life of the process. Dispose
        // unhooks, and the container calls it from App.OnExit and UiSession.Dispose, both of which
        // already dispose the provider. Without it a harness run's whole service graph would
        // survive its own run and a later system event would marshal to a dead dispatcher.
        SystemEvents.UserPreferenceChanged += OnUserPreferenceChanged;
    }

    public SystemAppearance Current => Read();

    public event EventHandler? Changed;

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        SystemEvents.UserPreferenceChanged -= OnUserPreferenceChanged;
    }

    private void OnUserPreferenceChanged(object sender, UserPreferenceChangedEventArgs e)
    {
        // Raised on the SystemEvents pump thread, never ours. Everything below — SystemParameters,
        // and the subscribers, which recolour shared brushes and call DwmSetWindowAttribute — has
        // to be on the dispatcher, so nothing is read here.
        if (_disposed || _dispatcher.HasShutdownStarted || _dispatcher.HasShutdownFinished) return;

        _dispatcher.BeginInvoke(() =>
        {
            if (_disposed) return;

            // Windows sends this for the mouse pointer, the wallpaper and much else. Only a change
            // we can actually see is worth a repaint of every window.
            var now = Read();
            if (now == _last) return;

            _last = now;
            Changed?.Invoke(this, EventArgs.Empty);
        });
    }

    private static SystemAppearance Read() => new(
        IsDark: ReadAppsUseLightTheme() == 0,
        IsHighContrast: SystemParameters.HighContrast);

    /// <summary>1 (light) when the value is missing, unreadable, or not a DWORD — Windows' own
    /// default, and not worth an unhandled exception on a theme switch.</summary>
    private static int ReadAppsUseLightTheme()
    {
        try
        {
            return Registry.GetValue(PersonalizeKey, "AppsUseLightTheme", 1) is int value ? value : 1;
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or IOException or UnauthorizedAccessException)
        {
            return 1;
        }
    }
}
