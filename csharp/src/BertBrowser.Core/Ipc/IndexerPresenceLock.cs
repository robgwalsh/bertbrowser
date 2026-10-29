using System.Runtime.InteropServices;

namespace BertBrowser.Core.Ipc;

/// <summary>
/// The one object that answers "is an index helper running?", and the thing that stops two of them
/// running at once.
/// </summary>
/// <remarks>
/// <para>
/// The helper holds a named mutex for its whole life; the app asks whether that name exists. Both
/// halves matter and they are deliberately the same object:
/// </para>
/// <list type="bullet">
/// <item><b>Presence.</b> The helper now outlives the app, so an app starting up has to find out
/// whether one is already there before deciding to raise a UAC prompt. Waiting to see whether
/// somebody connects would answer eventually and wrongly under load; this answers immediately.</item>
/// <item><b>Exclusion.</b> Two elevated helpers would tail one volume's journal into one SQLite
/// file and fight over the same pipe name, and nothing else in the design prevents it — the
/// scheduled task and a click on the banner are two independent ways to start one.</item>
/// </list>
/// <para>
/// <b>The medium-integrity app can open a high-integrity process's mutex</b> because
/// <c>SYNCHRONIZE</c> is not a write right and mandatory policy is no-write-<em>up</em>. That is the
/// same asymmetry the pipe's direction rests on (see <c>IIndexTransport</c>), used the same way, and
/// it was verified against a real medium-integrity process rather than assumed. The explicit DACL
/// is still required: without one, the default admits the creator's token and not much else.
/// </para>
/// <para>
/// Raw P/Invoke rather than <c>MutexAcl.Create</c>: <c>System.Threading.AccessControl</c> is not
/// part of plain <c>net10.0</c>, and the indexer deliberately targets that rather than
/// <c>net10.0-windows</c> because it draws nothing.
/// </para>
/// </remarks>
public static class IndexerPresenceLock
{
    private const uint SYNCHRONIZE = 0x00100000;
    private const int ERROR_ALREADY_EXISTS = 183;
    private const uint SDDL_REVISION_1 = 1;

    /// <summary>
    /// The mutex name for a user's helper.
    /// </summary>
    /// <remarks>
    /// <c>Local\</c>, not <c>Global\</c>: the helper and the app live in one interactive logon
    /// session — including when the helper is started by the sign-in task — and a session-scoped
    /// name keeps two users' helpers from ever meeting. The same choice <c>SingleInstance</c> makes.
    /// </remarks>
    public static string NameFor(string userSid) => $@"Local\BertBrowser.Indexer.{userSid}";

    /// <summary>
    /// The security descriptor the helper creates the mutex with: this user may synchronise on it,
    /// administrators and the system may do anything.
    /// </summary>
    /// <remarks>
    /// The user gets <c>SYNCHRONIZE</c> (0x00100000) and nothing more — enough to answer "does it
    /// exist?", and not enough to acquire or release it. Nothing between two processes of one user
    /// is a security boundary, but there is no reason to hand out more than the question needs.
    /// </remarks>
    public static string Sddl(string userSid) =>
        $"D:(A;;0x{SYNCHRONIZE:X8};;;{userSid})(A;;GA;;;BA)(A;;GA;;;SY)";

    /// <summary>
    /// Claims the name for this process, or returns null when another helper already holds it.
    /// </summary>
    /// <remarks>
    /// Disposing releases it. The handle is the claim, not ownership of the mutex — nothing ever
    /// waits on it, so there is no lock to acquire and nothing to abandon if this process is killed.
    /// The kernel drops the name when the last handle closes, however the process ended.
    /// </remarks>
    public static IDisposable? TryAcquire(string userSid)
    {
        if (!IsAcceptableSid(userSid))
            throw new ArgumentException("Not a SID this app would have produced.", nameof(userSid));

        var descriptor = IntPtr.Zero;
        try
        {
            if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    Sddl(userSid), SDDL_REVISION_1, out descriptor, IntPtr.Zero))
            {
                return null;
            }

            var attributes = new SecurityAttributes
            {
                nLength = Marshal.SizeOf<SecurityAttributes>(),
                lpSecurityDescriptor = descriptor,
                bInheritHandle = false,
            };

            var handle = CreateMutexW(ref attributes, false, NameFor(userSid));
            var error = Marshal.GetLastWin32Error();

            if (handle == IntPtr.Zero) return null;
            if (error == ERROR_ALREADY_EXISTS)
            {
                // We opened somebody else's. Let go of it rather than holding a handle that would
                // keep the name alive after they exit.
                CloseHandle(handle);
                return null;
            }

            return new Claim(handle);
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return null;
        }
        finally
        {
            if (descriptor != IntPtr.Zero) LocalFree(descriptor);
        }
    }

    /// <summary>True when a helper is running for this user.</summary>
    public static bool IsHeld(string userSid)
    {
        if (!IsAcceptableSid(userSid)) return false;

        try
        {
            var handle = OpenMutexW(SYNCHRONIZE, false, NameFor(userSid));
            if (handle == IntPtr.Zero) return false;

            CloseHandle(handle);
            return true;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return false;
        }
    }

    /// <summary>
    /// A SID in the form <c>ConvertSidToStringSid</c> produces, and nothing else.
    /// </summary>
    /// <remarks>
    /// This string is interpolated into both an SDDL string and a kernel object name. A value
    /// carrying <c>)</c> or <c>\</c> would rewrite the descriptor or name a different object, so it
    /// is checked here rather than trusted — the same discipline <c>IndexerArguments</c> applies to
    /// everything that reaches the elevated process.
    /// </remarks>
    public static bool IsAcceptableSid(string? candidate)
    {
        if (candidate is not { Length: >= 3 and <= 184 }) return false;
        if (!candidate.StartsWith("S-", StringComparison.Ordinal)) return false;
        if (candidate[^1] == '-') return false;

        // Past the "S-", a SID is only digits and the hyphens between them.
        foreach (var c in candidate.AsSpan(2))
        {
            if (c is not ('-' or >= '0' and <= '9')) return false;
        }

        return true;
    }

    private sealed class Claim : IDisposable
    {
        private IntPtr _handle;

        public Claim(IntPtr handle) => _handle = handle;

        public void Dispose()
        {
            if (_handle == IntPtr.Zero) return;
            CloseHandle(_handle);
            _handle = IntPtr.Zero;
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct SecurityAttributes
    {
        public int nLength;
        public IntPtr lpSecurityDescriptor;
        [MarshalAs(UnmanagedType.Bool)] public bool bInheritHandle;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateMutexW(
        ref SecurityAttributes attributes, [MarshalAs(UnmanagedType.Bool)] bool initialOwner, string name);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr OpenMutexW(
        uint desiredAccess, [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string sddl, uint revision, out IntPtr descriptor, IntPtr size);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);
}
