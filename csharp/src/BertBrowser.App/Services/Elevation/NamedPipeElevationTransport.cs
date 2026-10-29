using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Elevation;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Services.Elevation;

/// <summary>
/// The pipe one elevated operation talks over.
/// </summary>
/// <remarks>
/// <para>
/// <b>The app is the server and the elevated helper is the client</b>, which is the reverse of the
/// obvious arrangement and the same choice the index pipe makes. A pipe created by a high-integrity
/// process carries a High mandatory label, and mandatory policy forbids writing <em>up</em> — so a
/// medium-integrity app could not write to a pipe its own helper had created. Creating it here makes
/// the helper's connection a write-down, always permitted, and no labelling code is needed.
/// </para>
/// <para>
/// Real buffer sizes, and <c>PipeOptions.Asynchronous</c>: a duplex pipe with zero-size buffers
/// deadlocks the moment both ends greet each other, and a non-overlapped handle serialises I/O so a
/// blocking read blocks the peer's writes. Both were found the hard way on the index pipe.
/// </para>
/// <para>
/// The name carries a 128-bit nonce for the reason the single-instance endpoint does: pipe names are
/// one machine-wide, first-come namespace with no per-user partitioning, so a predictable name is
/// one another signed-in account could take first.
/// </para>
/// </remarks>
public sealed class NamedPipeElevationTransport : IElevationTransport
{
    private const int InBufferSize = 16 * 1024;
    private const int OutBufferSize = 16 * 1024;

    private readonly NamedPipeServerStream _server;

    public NamedPipeElevationTransport()
    {
        Endpoint = ElevatorArguments.PipePrefix + Nonce();

        var self = WindowsIdentity.GetCurrent().User
            ?? throw new InvalidOperationException("No user SID for the current process.");

        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            self,
            PipeAccessRights.ReadWrite | PipeAccessRights.CreateNewInstance,
            AccessControlType.Allow));

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

    /// <summary>Waits for the helper to connect, then checks it is the one we started and running as
    /// us. Never a naked <c>WaitForConnection</c>: a UAC prompt left sitting on screen would park the
    /// thread until it was answered.</summary>
    public Stream? Accept(int processId, TimeSpan timeout)
    {
        try
        {
            var connection = _server.WaitForConnectionAsync();
            if (!connection.Wait(timeout)) return null;
        }
        catch (Exception ex) when (
            ex is IOException or ObjectDisposedException or InvalidOperationException or AggregateException)
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
                server.GetImpersonationUserName(),
                WindowsIdentity.GetCurrent().Name);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException)
        {
            return false;
        }
    }

    private static bool IsTheProcessWeLaunched(
        NamedPipeServerStream server, int processId)
    {
        try
        {
            return GetNamedPipeClientProcessId(server.SafePipeHandle, out var client) &&
                   client == (uint)processId;
        }
        catch (Exception ex) when (
            ex is EntryPointNotFoundException or DllNotFoundException or ObjectDisposedException)
        {
            return false;
        }
    }

    private static string Nonce() =>
        Convert.ToHexString(RandomNumberGenerator.GetBytes(16));

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(
        SafePipeHandle pipe, out uint clientProcessId);

    public void Dispose() => _server.Dispose();
}

/// <summary>One transport per operation: this helper is one-shot, so its pipe is too.</summary>
public sealed class NamedPipeElevationTransportFactory : IElevationTransportFactory
{
    public IElevationTransport Create() => new NamedPipeElevationTransport();
}
