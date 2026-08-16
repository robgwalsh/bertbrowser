using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.App.Services.Indexing;

/// <summary>
/// The listening end of the index pipe, in this — the unprivileged — process.
/// </summary>
/// <remarks>
/// <para>
/// <b>The app listens and the helper connects, and that is not arbitrary.</b> A named pipe created
/// by a high-integrity process carries a High mandatory label, and mandatory policy forbids writing
/// up, so this medium-integrity process could not write to a pipe its own helper had created —
/// talking to it would mean setting labels by hand. Creating the pipe here makes the helper's
/// connection a write-<em>down</em>, which is always allowed, and the problem disappears.
/// </para>
/// <para>
/// The name carries a random nonce as well as the user's SID, so a fresh attempt after a failure
/// never collides with an endpoint the previous one left behind.
/// </para>
/// <para>
/// Two checks on the peer, and they answer different questions. The DACL and the account
/// comparison establish it is this user; <c>GetNamedPipeClientProcessId</c> establishes it is the
/// process we launched, rather than another of this user's own that raced for the name. Neither is
/// a security boundary — nothing between two processes of one user is — but the second is what
/// stops the elevated helper being adopted by something that never started it.
/// </para>
/// </remarks>
public sealed class NamedPipeIndexTransport : IIndexTransport
{
    /// <summary>
    /// Real buffers, and this is load-bearing rather than tuning.
    /// </summary>
    /// <remarks>
    /// A pipe created with zero-size buffers holds nothing: every write blocks until the peer
    /// reads it. On a duplex pipe where both ends greet each other that is a deadlock, and it is
    /// exactly the one this hit — the helper blocked sending <c>Ready</c>, so it never reached its
    /// read loop, while the app blocked sending <c>Hello</c> waiting for a reader that was never
    /// coming. <c>SingleInstance</c> gets away with zero because its pipe is one-directional and
    /// the server never writes a byte; do not copy that here. The sizes only need to hold a short
    /// burst of one-line messages.
    /// </remarks>
    private const int InBufferSize = 16 * 1024;
    private const int OutBufferSize = 4 * 1024;

    private readonly NamedPipeServerStream _server;

    public NamedPipeIndexTransport()
    {
        Endpoint = $"BertBrowser.Index.{KeyForCurrentUser()}.{Nonce()}";

        var self = WindowsIdentity.GetCurrent().User
            ?? throw new InvalidOperationException("No user SID for the current process.");

        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            self, PipeAccessRights.ReadWrite | PipeAccessRights.CreateNewInstance, AccessControlType.Allow));

        _server = NamedPipeServerStreamAcl.Create(
            Endpoint,
            PipeDirection.InOut,
            maxNumberOfServerInstances: 1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous,
            InBufferSize,
            OutBufferSize,
            security);
    }

    public string Endpoint { get; }

    public Stream? Accept(int processId, TimeSpan timeout)
    {
        try
        {
            // WaitForConnection blocks with no deadline of its own, and a helper that never arrives
            // (a UAC prompt left sitting) must not park this thread for the session.
            var connection = _server.WaitForConnectionAsync();
            if (!connection.Wait(timeout)) return null;
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException
                                      or InvalidOperationException or AggregateException)
        {
            return null;
        }

        if (!IsOurOwnUser(_server) || !IsTheProcessWeLaunched(_server, processId))
        {
            _server.Disconnect();
            return null;
        }

        return _server;
    }

    private static bool IsOurOwnUser(NamedPipeServerStream server)
    {
        try
        {
            return PipeIdentity.SameAccount(
                server.GetImpersonationUserName(), WindowsIdentity.GetCurrent().Name);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException)
        {
            return false;
        }
    }

    private static bool IsTheProcessWeLaunched(NamedPipeServerStream server, int processId)
    {
        try
        {
            return GetNamedPipeClientProcessId(server.SafePipeHandle, out var clientProcessId) &&
                   clientProcessId == (uint)processId;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException
                                      or ObjectDisposedException)
        {
            return false;
        }
    }

    private static string Nonce() => Convert.ToHexString(RandomNumberGenerator.GetBytes(16));

    private static string KeyForCurrentUser()
    {
        try
        {
            if (WindowsIdentity.GetCurrent().User is { } sid) return sid.Value;
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or InvalidOperationException)
        {
        }
        return "default";
    }

    public void Dispose() => _server.Dispose();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(SafeHandle pipe, out uint clientProcessId);
}

/// <summary>Makes a fresh pipe per attempt, so a retry never lands on a stale endpoint.</summary>
public sealed class NamedPipeIndexTransportFactory : IIndexTransportFactory
{
    public IIndexTransport Create() => new NamedPipeIndexTransport();
}
