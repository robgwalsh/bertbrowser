using System.Runtime.InteropServices;
using System.IO.Pipes;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Indexer;

/// <summary>
/// Everything this process knows about the one that started it: whether the pipe it connected to
/// really belongs to that process, and when that process goes away.
/// </summary>
/// <remarks>
/// <para>
/// The pipe's DACL already admits only this user, and nothing between two processes of one user is
/// a security boundary. What <see cref="OwnsPipe"/> adds is different: it establishes that the
/// endpoint is the process that launched us, rather than another of the user's own that guessed or
/// raced for the name. A helper that attached to the wrong server would index on behalf of
/// something that never asked.
/// </para>
/// <para>
/// <see cref="WaitForExit"/> is the belt to the pipe's braces. Losing the pipe is the primary
/// signal and covers a crash, since the kernel breaks it however the peer ends; this covers the
/// exotic case where a duplicated handle keeps the pipe open after the app is gone.
/// </para>
/// </remarks>
internal static class ParentProcess
{
    private const int SYNCHRONIZE = 0x00100000;
    private const uint WAIT_OBJECT_0 = 0;

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafeHandle pipe, out uint serverProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern SafeWaitHandle OpenProcess(int desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(SafeWaitHandle handle, uint milliseconds);

    /// <summary>True when <paramref name="pipe"/>'s server end belongs to <paramref name="expectedProcessId"/>.</summary>
    internal static bool OwnsPipe(NamedPipeClientStream pipe, int expectedProcessId)
    {
        try
        {
            if (!GetNamedPipeServerProcessId(pipe.SafePipeHandle, out var serverProcessId))
                return false;

            return serverProcessId == (uint)expectedProcessId;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return false;
        }
    }

    /// <summary>
    /// Blocks until the given process exits, then runs <paramref name="onExit"/>. Returns
    /// immediately if the process cannot be opened, which on this path means it has already gone.
    /// </summary>
    internal static void WatchForExit(int processId, Action onExit)
    {
        var thread = new Thread(() =>
        {
            using var handle = OpenProcess(SYNCHRONIZE, false, processId);
            if (handle.IsInvalid)
            {
                handle.SetHandleAsInvalid();
                onExit();
                return;
            }

            if (WaitForSingleObject(handle, uint.MaxValue) == WAIT_OBJECT_0)
                onExit();
        })
        {
            IsBackground = true,
            Name = "bertbrowser parent watchdog",
        };
        thread.Start();
    }
}
