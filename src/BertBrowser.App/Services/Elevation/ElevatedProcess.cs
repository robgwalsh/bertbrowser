using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Services.Elevation;

internal enum ElevatedStart
{
    Started,

    /// <summary>The user answered the prompt with No — <c>ERROR_CANCELLED</c>.</summary>
    Declined,

    Failed,
}

internal readonly record struct ElevatedStartResult(
    ElevatedStart Outcome, SafeProcessHandle? Handle = null, int ProcessId = 0, string Detail = "")
{
    internal static ElevatedStartResult Started(SafeProcessHandle handle, int processId) =>
        new(ElevatedStart.Started, handle, processId);

    internal static readonly ElevatedStartResult Declined = new(ElevatedStart.Declined);

    internal static ElevatedStartResult Failed(string detail) =>
        new(ElevatedStart.Failed, Detail: detail);
}

/// <summary>
/// Raising a UAC prompt and starting a helper behind it. The one place in the app that asks Windows
/// for an administrator token, shared by both helpers.
/// </summary>
/// <remarks>
/// <para>
/// <c>ShellExecuteEx</c> with the <c>runas</c> verb rather than <c>Process.Start</c>, for the
/// handle: <c>SEE_MASK_NOCLOSEPROCESS</c> hands back the process, which is what lets a pipe verify
/// its peer is really the process we started, and lets a shutdown wait on it. A declined prompt
/// comes back as <c>ERROR_CANCELLED</c>, distinct from every other failure, because "you said no"
/// and "it did not work" deserve different words in the status bar.
/// </para>
/// <para>
/// Shared rather than copied, and that matters most for <see cref="CanElevate"/>: two copies of the
/// token-elevation-type reasoning below is exactly the kind of duplication that goes stale in one
/// place and misleads for years.
/// </para>
/// </remarks>
internal static class ElevatedProcess
{
    private const int SW_HIDE = 0;
    private const int SEE_MASK_NOCLOSEPROCESS = 0x00000040;
    private const int SEE_MASK_NOASYNC = 0x00000100;
    private const int SEE_MASK_FLAG_NO_UI = 0x00000400;
    private const int ERROR_CANCELLED = 1223;

    /// <summary>
    /// Whether this account could elevate if asked.
    /// </summary>
    /// <remarks>
    /// <b>Not <c>IsInRole(Administrator)</c>.</b> An administrator running normally holds a
    /// <em>filtered</em> token in which the Administrators group is marked deny-only, so
    /// <c>IsInRole</c> answers false for exactly the people who can elevate — and the app would tell
    /// every one of them that their account is not an administrator, without ever offering the
    /// prompt. The elevation type is the question actually being asked: <c>Limited</c> means a split
    /// token and therefore an admin, <c>Full</c> means already elevated, and <c>Default</c> means no
    /// split token at all, where group membership is finally meaningful.
    /// </remarks>
    internal static bool CanElevate
    {
        get
        {
            try
            {
                using var identity = WindowsIdentity.GetCurrent();
                return GetElevationType(identity) switch
                {
                    TokenElevationType.Limited => true,
                    TokenElevationType.Full => true,
                    _ => new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator),
                };
            }
            catch (Exception ex) when (
                ex is UnauthorizedAccessException or InvalidOperationException or Win32Exception)
            {
                // Indeterminate. Offering the prompt is the better failure: the worst case is one
                // dialog, against silently never being able to do something the user could have.
                return true;
            }
        }
    }

    /// <summary>The SID of the account this process is running as — which is also the account an
    /// elevated child will run as, since UAC hands the same user a different token.</summary>
    internal static string? CurrentUserSid
    {
        get
        {
            try
            {
                using var identity = WindowsIdentity.GetCurrent();
                return identity.User?.Value;
            }
            catch (Exception ex) when (ex is UnauthorizedAccessException or InvalidOperationException)
            {
                return null;
            }
        }
    }

    /// <param name="exePath">Resolved beside this executable by the caller and never by name: what
    /// is being launched is the one thing that gets an administrator token, so it must not be
    /// findable on <c>PATH</c>.</param>
    internal static ElevatedStartResult Start(string exePath, string arguments, string missingDetail)
    {
        // Worth being specific about: a build that never copied the helper beside the app fails
        // here, before any prompt is raised, and "unavailable" alone gives no hint that the cause is
        // a missing file rather than anything to do with elevation.
        if (!File.Exists(exePath)) return ElevatedStartResult.Failed(missingDetail);

        var info = new ShellExecuteInfo
        {
            cbSize = Marshal.SizeOf<ShellExecuteInfo>(),
            fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb = "runas",
            lpFile = exePath,
            lpParameters = arguments,
            lpDirectory = AppContext.BaseDirectory,
            nShow = SW_HIDE,
        };

        if (!ShellExecuteEx(ref info))
        {
            var error = Marshal.GetLastWin32Error();
            return error == ERROR_CANCELLED
                ? ElevatedStartResult.Declined
                : ElevatedStartResult.Failed(new Win32Exception(error).Message);
        }

        var handle = new SafeProcessHandle(info.hProcess, ownsHandle: true);
        var processId = GetProcessId(info.hProcess);
        if (processId == 0)
        {
            handle.Dispose();
            return ElevatedStartResult.Failed("it started but could not be identified");
        }

        return ElevatedStartResult.Started(handle, processId);
    }

    /// <summary>
    /// Waits on the handle we were given rather than reopening the process: a medium-integrity
    /// process cannot count on opening a high-integrity one at all, and this handle already exists.
    /// </summary>
    internal static void WaitForExit(SafeProcessHandle? handle, TimeSpan timeout)
    {
        if (handle is null || handle.IsInvalid) return;

        using var wait = new ManualResetEvent(false)
        {
            SafeWaitHandle = new SafeWaitHandle(handle.DangerousGetHandle(), ownsHandle: false),
        };
        wait.WaitOne(timeout);
    }

    private enum TokenElevationType
    {
        Default = 1,
        Full = 2,
        Limited = 3,
    }

    private static TokenElevationType GetElevationType(WindowsIdentity identity)
    {
        var size = sizeof(int);
        var buffer = Marshal.AllocHGlobal(size);
        try
        {
            // TokenElevationType == 18
            if (!GetTokenInformation(identity.Token, 18, buffer, size, out _))
                throw new Win32Exception(Marshal.GetLastWin32Error());

            return (TokenElevationType)Marshal.ReadInt32(buffer);
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct ShellExecuteInfo
    {
        public int cbSize;
        public int fMask;
        public IntPtr hwnd;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpVerb;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpFile;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpParameters;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpDirectory;
        public int nShow;
        public IntPtr hInstApp;
        public IntPtr lpIDList;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpClass;
        public IntPtr hkeyClass;
        public int dwHotKey;
        public IntPtr hIcon;
        public IntPtr hProcess;
    }

    [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ShellExecuteEx(ref ShellExecuteInfo info);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern int GetProcessId(IntPtr process);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetTokenInformation(IntPtr token, int informationClass,
        IntPtr information, int informationLength, out int returnLength);
}
