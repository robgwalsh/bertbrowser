using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
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
/// Entirely best-effort: every call is a hint the window manager may refuse, and a refusal costs a
/// highlighted taskbar button rather than a lost request.
/// </para>
/// </remarks>
internal static class ForegroundWindow
{
    private const int SwRestore = 9;

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
    /// worthless if it opens in a window that stays behind what the user was looking at.
    /// </summary>
    public static void Raise(Window window)
    {
        if (window.WindowState == WindowState.Minimized)
            window.WindowState = WindowState.Normal;

        var handle = new WindowInteropHelper(window).Handle;
        if (handle == 0)
        {
            window.Activate();
            return;
        }

        // Restore through the OS as well: a window minimized by the shell rather than by WPF can
        // have the two disagree, and SetForegroundWindow on a still-minimized window does nothing.
        ShowWindow(handle, SwRestore);
        SetForegroundWindow(handle);

        // WPF's own bookkeeping — focus within the window, activation state — still needs telling.
        window.Activate();
    }
}
