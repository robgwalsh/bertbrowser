using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using BertBrowser.Core.Services.Foreground;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Interop;

/// <summary>
/// Bringing the running copy to the front when a second launch hands its command line over.
/// </summary>
/// <remarks>
/// <para>
/// <b>Windows will not let a background process raise its own window</b>, and this is the reason a
/// single-instance hand-off looks like it did nothing: the second copy writes to the pipe and
/// exits, the running copy calls <c>Activate()</c>, and the foreground lock silently downgrades it
/// to a flashing taskbar button. The window really did receive the request and really did open the
/// folder — behind whatever the user was looking at.
/// </para>
/// <para>
/// The fix is that the process which <i>has</i> the right gives it away. The second copy was just
/// started by the shell in response to a double-click, so it holds foreground rights;
/// <c>AllowSetForegroundWindow</c> transfers them to the copy that is going to do the work, and its
/// <c>SetForegroundWindow</c> then succeeds. The target is identified with
/// <c>GetNamedPipeServerProcessId</c> off the pipe already connected to it, rather than
/// <c>ASFW_ANY</c> — there is an exact answer available, so the permission is granted to exactly
/// one process.
/// </para>
/// <para>
/// This is <b>not</b> the <c>AllowSetForegroundWindow</c> dance that came out with
/// <c>ShellLauncher</c> and must not come back. That one existed to launch other people's programs
/// through <c>explorer.exe</c> and borrow a lesser token; it went with the manifest change. This is
/// one process handing foreground rights to another copy of itself, which is what the API is for.
/// </para>
/// <para>
/// Because all of that defeats the foreground lock <em>by design</em>, <see cref="Raise"/> asks
/// <see cref="ForegroundRaiseRules"/> first whether this is a moment to interrupt at all — see there
/// for how a file manager ends up on top of someone's full-screen video.
/// </para>
/// <para>
/// Entirely best-effort: every call is a hint the window manager may refuse, and a refusal costs a
/// highlighted taskbar button rather than a lost request.
/// </para>
/// </remarks>
internal static class ForegroundWindow
{
    private const int SwRestore = 9;

    private const uint MonitorDefaultToNearest = 2;

    // FLASHW_ALL (caption and taskbar button) until the window comes to the foreground — the same
    // signal the window manager raises by itself when it refuses a SetForegroundWindow. Declining
    // the raise means declining that too, so it has to be asked for explicitly.
    private const uint FlashAll = 3;
    private const uint FlashTimerUntilForeground = 12;

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AllowSetForegroundWindow(int dwProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint processId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(nint hWnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ShowWindow(nint hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern nint GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(nint hWnd, out uint processId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowRect(nint hWnd, out NativeRect rect);

    [DllImport("user32.dll")]
    private static extern nint MonitorFromWindow(nint hWnd, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetMonitorInfo(nint monitor, ref MonitorInfo info);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool FlashWindowEx(ref FlashInfo info);

    [DllImport("shell32.dll")]
    private static extern int SHQueryUserNotificationState(out UserNotificationState state);

    /// <summary>
    /// Hands this process's right to take the foreground to whichever process is serving
    /// <paramref name="pipe"/>. Call before writing the request, so the permission is in place by
    /// the time the other copy acts on it.
    /// </summary>
    public static void GrantTo(SafePipeHandle pipe)
    {
        try
        {
            if (GetNamedPipeServerProcessId(pipe, out var processId) && processId != 0)
                AllowSetForegroundWindow((int)processId);
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
        }
    }

    /// <summary>
    /// Raises <paramref name="window"/>, restoring it first if it was minimized — a request is
    /// worthless if it opens in a window that stays behind what the user was looking at — unless
    /// something is full screen, in which case the window stays exactly where it is and only its
    /// taskbar button flashes. The restore sits inside that decision deliberately: restoring a
    /// minimized window puts it on screen whether or not it also takes the foreground.
    /// </summary>
    public static void Raise(Window window)
    {
        var handle = new WindowInteropHelper(window).Handle;
        if (handle == 0)
        {
            // No handle yet: nothing to be in front of anything, and nothing to flash either.
            window.Activate();
            return;
        }

        if (Decide(handle) == RaiseAction.Flash)
        {
            Flash(handle);
            return;
        }

        if (window.WindowState == WindowState.Minimized)
            window.WindowState = WindowState.Normal;

        // Restore through the OS as well: a window minimized by the shell rather than by WPF can
        // have the two disagree, and SetForegroundWindow on a still-minimized window does nothing.
        ShowWindow(handle, SwRestore);
        SetForegroundWindow(handle);

        // WPF's own bookkeeping — focus within the window, activation state — still needs telling.
        window.Activate();
    }

    /// <summary>Gathers what <see cref="ForegroundRaiseRules"/> decides on. Every failure answers
    /// <see cref="RaiseAction.Raise"/>: a request the user made must not be lost to an API that
    /// would not say.</summary>
    private static RaiseAction Decide(nint own)
    {
        try
        {
            var foreground = GetForegroundWindow();
            if (foreground == 0) return RaiseAction.Raise;

            GetWindowThreadProcessId(foreground, out var processId);

            return ForegroundRaiseRules.Decide(
                QueryNotificationState(),
                foregroundIsOurs: foreground == own || processId == (uint)Environment.ProcessId,
                foregroundCoversMonitor: CoversItsMonitor(foreground));
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return RaiseAction.Raise;
        }
    }

    private static UserNotificationState QueryNotificationState()
    {
        try
        {
            return SHQueryUserNotificationState(out var state) == 0
                ? state
                : UserNotificationState.Unknown;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return UserNotificationState.Unknown;
        }
    }

    /// <summary>Whether <paramref name="window"/> fills its monitor's whole bounds. Compared
    /// against <c>rcMonitor</c> and not <c>rcWork</c>, so an ordinary maximized window — which stops
    /// at the taskbar, as this app's own does — is not mistaken for a full-screen one.</summary>
    private static bool CoversItsMonitor(nint window)
    {
        if (!GetWindowRect(window, out var rect)) return false;

        var monitor = MonitorFromWindow(window, MonitorDefaultToNearest);
        if (monitor == 0) return false;

        var info = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
        if (!GetMonitorInfo(monitor, ref info)) return false;

        return rect.Left <= info.Monitor.Left && rect.Top <= info.Monitor.Top &&
               rect.Right >= info.Monitor.Right && rect.Bottom >= info.Monitor.Bottom;
    }

    private static void Flash(nint window)
    {
        var info = new FlashInfo
        {
            Size = (uint)Marshal.SizeOf<FlashInfo>(),
            Window = window,
            Flags = FlashAll | FlashTimerUntilForeground,
        };

        FlashWindowEx(ref info);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MonitorInfo
    {
        public int Size;
        public NativeRect Monitor;
        public NativeRect Work;
        public uint Flags;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FlashInfo
    {
        public uint Size;
        public nint Window;
        public uint Flags;
        public uint Count;
        public uint Timeout;
    }
}
