using System.Text;
using System.Runtime.InteropServices;
using System.IO.Pipes;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Ipc;

/// <summary>
/// Everything an elevated helper knows about the app that started it: whether the pipe it connected
/// to really belongs to that process, and when that process goes away.
/// </summary>
/// <remarks>
/// <para>
/// Shared by both helpers rather than copied into each, because it is the check every one of them
/// has to get right and there should be one of it to audit. It lives in Core for the reason
/// <c>CopyNative</c> does: it is pure P/Invoke and draws nothing.
/// </para>
/// <para>
/// The pipe's DACL already admits only this user, and nothing between two processes of one user is
/// a security boundary. What <see cref="OwnsPipe"/> adds is different: it establishes that the
/// endpoint is the process that launched us, rather than another of the user's own that guessed or
/// raced for the name. A helper that attached to the wrong server would work on behalf of something
/// that never asked.
/// </para>
/// <para>
/// <see cref="WaitForExit"/> is the belt to the pipe's braces. Losing the pipe is the primary
/// signal and covers a crash, since the kernel breaks it however the peer ends; this covers the
/// exotic case where a duplicated handle keeps the pipe open after the app is gone.
/// </para>
/// </remarks>
public static class PipeOwner
{
    private const int SYNCHRONIZE = 0x00100000;
    private const int PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    private const uint WAIT_OBJECT_0 = 0;

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafeHandle pipe, out uint serverProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern SafeWaitHandle OpenProcess(int desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(SafeWaitHandle handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode, EntryPoint = "QueryFullProcessImageNameW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool QueryFullProcessImageNameW(
        SafeWaitHandle process, int flags, StringBuilder name, ref int size);

    /// <summary>True when <paramref name="pipe"/>'s server end belongs to <paramref name="expectedProcessId"/>.</summary>
    public static bool OwnsPipe(NamedPipeClientStream pipe, int expectedProcessId)
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
    /// The full path of a running process's executable, or null when it cannot be read.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Used by the file-operation helper to check that the process on the other end of its pipe is
    /// the copy of BertBrowser sitting beside it, and deliberately not by the index helper. A
    /// high-integrity process may open a medium-integrity one for
    /// <c>PROCESS_QUERY_LIMITED_INFORMATION</c>, so this direction works where the reverse would not.
    /// </para>
    /// <para>
    /// <b>It is a coherence check, not a boundary, and writing it down as one would be the mistake
    /// to avoid.</b> Nothing between two processes of a single user is a security boundary: a
    /// program running as this user could copy the command line, or simply raise its own prompt.
    /// What protects the user is the UAC dialog naming the helper. This rules out confusion — a
    /// stale build, a raced name — and nothing more.
    /// </para>
    /// </remarks>
    public static string? ImagePathOf(int processId)
    {
        try
        {
            using var handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, processId);
            if (handle.IsInvalid)
            {
                handle.SetHandleAsInvalid();
                return null;
            }

            var name = new StringBuilder(1024);
            var size = name.Capacity;
            return QueryFullProcessImageNameW(handle, 0, name, ref size) ? name.ToString() : null;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return null;
        }
    }

    /// <summary>
    /// Blocks until the given process exits, then runs <paramref name="onExit"/>. Returns
    /// immediately if the process cannot be opened, which on this path means it has already gone.
    /// </summary>
    public static void WatchForExit(int processId, Action onExit)
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
