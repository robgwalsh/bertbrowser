using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
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
/// connection a write-<em>down</em>, which is always allowed, and the problem disappears. That
/// argument is why the helper outliving the app did <em>not</em> turn it into a server: an
/// always-on elevated process that listens is a much larger thing than one that only ever dials out.
/// </para>
/// <para>
/// <b>The name is well-known rather than nonced, and that is what makes attaching possible.</b> It
/// used to carry a random nonce so a retry could not land on an endpoint a previous attempt had
/// left behind. That stopped being the right trade once the helper began outliving apps: a helper
/// started under a previous app has to find <em>this</em> one, and it cannot guess a nonce. The
/// server is therefore created once and held for the app's life — while no helper is attached the
/// pipe must still exist, or a helper started later has nothing to rendezvous with.
/// </para>
/// <para>
/// Three checks on the peer, answering different questions. The DACL and the account comparison
/// establish it is this user. When we launched the helper ourselves,
/// <c>GetNamedPipeClientProcessId</c> establishes it is the process we started. When we merely
/// attached to one that was already running there is no such process id, so
/// <see cref="PeerIntegrity"/> establishes that whatever connected at least holds an administrator
/// token — which an ordinary program of this user does not. None of these is a security boundary —
/// nothing between two processes of one user is — but together they stop the app mistaking
/// something else for an index helper and mirroring state nothing is maintaining.
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
    private readonly object _gate = new();

    /// <summary>
    /// The wait in progress, kept across calls.
    /// </summary>
    /// <remarks>
    /// <see cref="Accept"/> can time out and be called again — that is the whole shape of "look for
    /// a helper briefly, and keep listening in case one starts later". Two concurrent
    /// <c>WaitForConnectionAsync</c> calls on one server throw, so the second attempt has to wait
    /// the task the first one started rather than begin another.
    /// </remarks>
    private Task? _pending;

    public NamedPipeIndexTransport()
    {
        Endpoint = IndexEndpoint.ForCurrentUser();

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

    public Stream? Accept(int? launchedProcessId, TimeSpan timeout)
    {
        try
        {
            Task pending;
            lock (_gate)
            {
                // A previous session left the server connected; it has to be let go before it can
                // wait again. This only ever bites on the second session, which is exactly where a
                // manual test stops looking.
                if (_pending is null && _server.IsConnected) _server.Disconnect();
                pending = _pending ??= _server.WaitForConnectionAsync();
            }

            if (!pending.Wait(timeout)) return null;

            lock (_gate) _pending = null;
            if (pending.IsFaulted) return null;
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException
                                      or InvalidOperationException or AggregateException)
        {
            lock (_gate) _pending = null;
            return null;
        }

        if (!IsAcceptablePeer(launchedProcessId))
        {
            try { _server.Disconnect(); } catch (Exception ex) when (ex is IOException or InvalidOperationException) { }
            return null;
        }

        // Never the server itself: the caller disposes the stream when a session ends, and the
        // server has to survive that to host the next one.
        return new PipeSession(_server);
    }

    private bool IsAcceptablePeer(int? launchedProcessId)
    {
        if (!IsOurOwnUser(_server)) return false;

        return launchedProcessId is { } processId
            ? IsTheProcessWeLaunched(_server, processId)
            : PeerIntegrity.ClientIsElevated(_server);
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

    public void Dispose() => _server.Dispose();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(SafeHandle pipe, out uint clientProcessId);
}

/// <summary>
/// One session's view of the shared pipe server: disposing it hangs up, and leaves the server
/// listening for the next helper.
/// </summary>
/// <remarks>
/// The client ends a session by disposing the stream it was given, which used to be the server
/// itself and used to be right — the pipe was thrown away after every attempt. Now the server has
/// to outlive every session it hosts, so what the client disposes must be this instead.
/// </remarks>
internal sealed class PipeSession : Stream
{
    private readonly NamedPipeServerStream _server;
    private bool _closed;

    public PipeSession(NamedPipeServerStream server) => _server = server;

    public override bool CanRead => !_closed && _server.CanRead;
    public override bool CanSeek => false;
    public override bool CanWrite => !_closed && _server.CanWrite;
    public override long Length => throw new NotSupportedException();
    public override long Position { get => 0; set => throw new NotSupportedException(); }
    public override void Flush() => _server.Flush();
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override int Read(byte[] buffer, int offset, int count) => _server.Read(buffer, offset, count);
    public override void Write(byte[] buffer, int offset, int count) => _server.Write(buffer, offset, count);

    protected override void Dispose(bool disposing)
    {
        if (!_closed)
        {
            _closed = true;
            // Hang up on this helper without destroying the endpoint the next one needs.
            try
            {
                if (_server.IsConnected) _server.Disconnect();
            }
            catch (Exception ex) when (ex is IOException or InvalidOperationException or ObjectDisposedException)
            {
            }
        }
        base.Dispose(disposing);
    }
}

/// <summary>
/// Holds the one endpoint for the app's life.
/// </summary>
/// <remarks>
/// A single instance, not one per attempt: the name is now well-known and
/// <c>maxNumberOfServerInstances</c> is 1, so a second server would simply fail to be created — and
/// the endpoint has to stay up between attempts anyway, so a helper started after a failed look can
/// still find it.
/// </remarks>
public sealed class NamedPipeIndexTransportFactory : IIndexTransportFactory, IDisposable
{
    private readonly object _gate = new();
    private NamedPipeIndexTransport? _transport;

    public bool TryCreate(out IIndexTransport? transport, out string error)
    {
        lock (_gate)
        {
            if (_transport is not null)
            {
                transport = _transport;
                error = "";
                return true;
            }

            try
            {
                _transport = new NamedPipeIndexTransport();
                transport = _transport;
                error = "";
                return true;
            }
            catch (IOException)
            {
                // The name is taken, which for a per-user name means another copy of this app owns
                // the index. Reported rather than thrown: it is an ordinary state, not a bug.
                transport = null;
                error = "another copy of BertBrowser is using it";
                return false;
            }
            catch (UnauthorizedAccessException)
            {
                transport = null;
                error = "another copy of BertBrowser is using it";
                return false;
            }
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            _transport?.Dispose();
            _transport = null;
        }
    }
}
