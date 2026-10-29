using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace BertBrowser.Core.Ipc;

/// <summary>
/// Whether the process on the other end of a pipe holds an administrator token.
/// </summary>
/// <remarks>
/// <para>
/// Used by the app to check that something claiming to be the index helper actually is one. It
/// matters because the helper now outlives the app: a session that <em>attaches</em> to a helper
/// this process did not launch cannot compare a process id against one it started, so
/// <c>GetNamedPipeClientProcessId</c> has nothing to be checked against.
/// </para>
/// <para>
/// <b>This works in the direction an image-path check does not.</b> A medium-integrity process
/// cannot open a high-integrity one to ask what it is running
/// (see <see cref="PipeOwner.ImagePathOf"/>), but it can impersonate a client that connected to its
/// own pipe and read that token's label — the helper connects with
/// <see cref="System.Security.Principal.TokenImpersonationLevel.Identification"/>, which permits
/// querying even though it permits nothing else. Verified against a real medium server and a real
/// elevated client rather than assumed.
/// </para>
/// <para>
/// <b>It is a coherence check, not a security boundary.</b> Nothing between two processes of one
/// user is one. What it rules out is another of this user's own ordinary programs being mistaken
/// for the helper and feeding this app an index state nothing is actually maintaining.
/// </para>
/// </remarks>
[SupportedOSPlatform("windows")]
public static class PeerIntegrity
{
    /// <summary>The mandatory label of an elevated process.</summary>
    private const int HighIntegrityRid = 0x3000;

    private const int TokenIntegrityLevel = 25;
    private const uint TokenQuery = 0x0008;

    /// <summary>
    /// True when the client connected to <paramref name="server"/> holds a High (or better) token.
    /// </summary>
    /// <remarks>
    /// False whenever the answer cannot be established, so an unreadable token is refused rather
    /// than assumed friendly.
    /// </remarks>
    public static bool ClientIsElevated(NamedPipeServerStream server)
    {
        try
        {
            var elevated = false;
            server.RunAsClient(() => elevated = CurrentThreadIsElevated());
            return elevated;
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException
                                      or UnauthorizedAccessException or ObjectDisposedException
                                      or EntryPointNotFoundException or DllNotFoundException)
        {
            return false;
        }
    }

    private static bool CurrentThreadIsElevated()
    {
        if (!OpenThreadToken(GetCurrentThread(), TokenQuery, true, out var token)) return false;

        try
        {
            // Asking with no buffer reports how large one has to be.
            GetTokenInformation(token, TokenIntegrityLevel, IntPtr.Zero, 0, out var needed);
            if (needed <= 0) return false;

            var buffer = Marshal.AllocHGlobal(needed);
            try
            {
                if (!GetTokenInformation(token, TokenIntegrityLevel, buffer, needed, out needed))
                    return false;

                // TOKEN_MANDATORY_LABEL is a SID_AND_ATTRIBUTES, whose first field is the SID.
                var sid = Marshal.ReadIntPtr(buffer);
                if (sid == IntPtr.Zero) return false;

                var count = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
                if (count == 0) return false;

                var rid = Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(count - 1)));
                return rid >= HighIntegrityRid;
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        finally
        {
            CloseHandle(token);
        }
    }

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool OpenThreadToken(IntPtr thread, uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool openAsSelf, out IntPtr token);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentThread();

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetTokenInformation(IntPtr token, int informationClass,
        IntPtr information, int length, out int returnLength);

    [DllImport("advapi32.dll")]
    private static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);

    [DllImport("advapi32.dll")]
    private static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);

    [DllImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);
}
