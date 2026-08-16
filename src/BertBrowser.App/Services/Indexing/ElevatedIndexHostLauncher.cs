using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Threading;
using BertBrowser.Core.Services.Mft;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Services.Indexing;

/// <summary>
/// Starts <c>BertBrowser.Indexer.exe</c> with an administrator token, which is the only elevation
/// this application performs and the only one it needs.
/// </summary>
/// <remarks>
/// <para>
/// <c>ShellExecuteEx</c> with the <c>runas</c> verb rather than <c>Process.Start</c>, for the
/// handle: <c>SEE_MASK_NOCLOSEPROCESS</c> hands back the process, which is what lets the pipe
/// verify the peer is really the process we started, and lets a shutdown wait on it. A declined
/// prompt comes back as <c>ERROR_CANCELLED</c>, distinct from every other failure, because
/// "you said no" and "it did not work" deserve different words in the status bar.
/// </para>
/// <para>
/// The helper is resolved beside this executable and never by name — it must not be found on
/// <c>PATH</c>, since what is being launched here is the one thing that gets an administrator
/// token.
/// </para>
/// </remarks>
public sealed class ElevatedIndexHostLauncher : IIndexHostLauncher, IDisposable
{
    private const int SW_HIDE = 0;
    private const int SEE_MASK_NOCLOSEPROCESS = 0x00000040;
    private const int SEE_MASK_NOASYNC = 0x00000100;
    private const int SEE_MASK_FLAG_NO_UI = 0x00000400;
    private const int ERROR_CANCELLED = 1223;

    private readonly string _helperPath;
    private readonly object _gate = new();
    private SafeProcessHandle? _process;
    private int _processId;

    public ElevatedIndexHostLauncher()
        : this(Path.Combine(AppContext.BaseDirectory, "BertBrowser.Indexer.exe"))
    {
    }

    internal ElevatedIndexHostLauncher(string helperPath) => _helperPath = helperPath;

    /// <inheritdoc/>
    /// <remarks>
    /// <b>Not <c>IsInRole(Administrator)</c>.</b> An administrator running normally holds a
    /// <em>filtered</em> token in which the Administrators group is marked deny-only, so
    /// <c>IsInRole</c> answers false for exactly the people who can elevate — and the app would
    /// tell every one of them that their account is not an administrator, without ever offering the
    /// prompt. The elevation type is the question actually being asked: <c>Limited</c> means a
    /// split token and therefore an admin, <c>Full</c> means already elevated, and <c>Default</c>
    /// means no split token at all, where group membership is finally meaningful.
    /// </remarks>
    public bool CanElevate
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
            catch (Exception ex) when (ex is UnauthorizedAccessException or InvalidOperationException or Win32Exception)
            {
                // Indeterminate. Offering the prompt is the better failure: the worst case is one
                // dialog, against silently never indexing for someone who could have.
                return true;
            }
        }
    }

    public IndexHostLaunchResult Launch(string pipeName, int parentProcessId)
    {
        // Worth being specific about: a build that never copied the helper beside the app fails
        // here, before any prompt is raised, and "unavailable" alone gives no hint that the cause
        // is a missing file rather than anything to do with elevation.
        if (!File.Exists(_helperPath))
            return IndexHostLaunchResult.Failed("the index helper is missing");

        var info = new ShellExecuteInfo
        {
            cbSize = Marshal.SizeOf<ShellExecuteInfo>(),
            fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb = "runas",
            lpFile = _helperPath,
            lpParameters = FormatArguments(pipeName, parentProcessId),
            lpDirectory = AppContext.BaseDirectory,
            nShow = SW_HIDE,
        };

        if (!ShellExecuteEx(ref info))
        {
            var error = Marshal.GetLastWin32Error();
            return error == ERROR_CANCELLED
                ? IndexHostLaunchResult.Declined
                : IndexHostLaunchResult.Failed(new Win32Exception(error).Message);
        }

        var handle = new SafeProcessHandle(info.hProcess, ownsHandle: true);
        var processId = GetProcessId(info.hProcess);
        if (processId == 0)
        {
            handle.Dispose();
            return IndexHostLaunchResult.Failed("the index helper started but could not be identified");
        }

        lock (_gate)
        {
            _process?.Dispose();
            _process = handle;
            _processId = processId;
        }

        return IndexHostLaunchResult.Started(processId);
    }

    /// <summary>
    /// Waits on the handle we were given rather than reopening the process: a medium-integrity
    /// process cannot count on opening a high-integrity one at all, and this handle already exists.
    /// </summary>
    public void WaitForExit(int processId, TimeSpan timeout)
    {
        SafeProcessHandle? handle;
        lock (_gate)
        {
            handle = _processId == processId ? _process : null;
        }
        if (handle is null || handle.IsInvalid) return;

        using var wait = new ManualResetEvent(false) { SafeWaitHandle = new SafeWaitHandle(handle.DangerousGetHandle(), ownsHandle: false) };
        wait.WaitOne(timeout);
    }

    /// <summary>
    /// Quoted, because the data directory can contain spaces — it is under the user's profile.
    /// Nothing here comes from a file being browsed; the pipe name is generated and the path is
    /// this app's own.
    /// </summary>
    private static string FormatArguments(string pipeName, int parentProcessId) =>
        $"--pipe \"{pipeName}\" --parent-pid {parentProcessId} --data-dir \"{AppPaths.DataDir.TrimEnd('\\')}\"";

    public void Dispose()
    {
        lock (_gate)
        {
            _process?.Dispose();
            _process = null;
        }
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
