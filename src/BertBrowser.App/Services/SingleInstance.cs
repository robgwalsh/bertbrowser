using System.IO;
using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;
using BertBrowser.Core.Cli;

namespace BertBrowser.App.Services;

/// <summary>
/// Keeps one copy of BertBrowser running per user, and gives a second launch a way to hand its
/// command line to the first instead of starting over.
/// </summary>
/// <remarks>
/// <para>
/// Worth having on its own merits, quite apart from the convenience: two copies each run their own
/// MFT indexer against the same SQLite database, and <c>DeleteExecutor.PurgeAbandonedStaging</c>
/// only skips batches under a day old <em>because</em> a second copy might be holding a pending
/// undo. One instance is what makes that assumption sound.
/// </para>
/// <para>
/// Both peers are elevated processes belonging to the same user, so the pipe needs no mandatory-label
/// work: a DACL admitting only the current user's SID is exactly right, and the default High label
/// is what we want. The client's identity is re-checked after connecting anyway.
/// </para>
/// <para>
/// Nothing here is a security boundary against the user themselves — it cannot be. What it does
/// bound is the <em>protocol</em>: one line, one request, "navigate to this path", validated by
/// <see cref="NavigationRequest.IsAcceptablePath"/>. Nothing on the wire can become a launch, a
/// file that gets written, or anything but a directory listing.
/// </para>
/// </remarks>
public sealed class SingleInstance : IDisposable
{
    /// <summary>Long enough to cross a busy machine, short enough that a wedged first instance does
    /// not stall the second one's startup.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromMilliseconds(500);

    private readonly string _pipeName;
    private readonly Mutex _mutex;
    private readonly CancellationTokenSource _stopping = new();
    private Thread? _listener;

    private SingleInstance(string pipeName, Mutex mutex, bool isFirst)
    {
        _pipeName = pipeName;
        _mutex = mutex;
        IsFirst = isFirst;
    }

    /// <summary>True for the copy that should actually start up.</summary>
    public bool IsFirst { get; }

    /// <summary>Raised on a background thread when another copy hands over a request.</summary>
    public event Action<CommandLineRequest>? RequestReceived;

    /// <summary>
    /// Claims the instance for this user. Names are per-user — two people signed in at once each get
    /// their own copy, which is the only sane reading of "single instance" on a shared machine.
    /// </summary>
    public static SingleInstance Claim()
    {
        var key = KeyForCurrentUser();
        // Local\ rather than Global\: this is a per-session, per-user thing, and Global\ would need
        // privileges we should not be relying on.
        var mutex = new Mutex(initiallyOwned: true, $"Local\\BertBrowser.{key}", out var isFirst);
        return new SingleInstance($"BertBrowser.{key}", mutex, isFirst);
    }

    /// <summary>Starts listening. Only the first instance should call this.</summary>
    public void StartListening()
    {
        if (!IsFirst) return;

        _listener = new Thread(Listen)
        {
            IsBackground = true,
            Name = "BertBrowser single-instance listener",
        };
        _listener.Start();
    }

    /// <summary>
    /// Hands <paramref name="request"/> to the copy already running. False when it could not be
    /// reached — the first instance may be mid-shutdown — in which case the caller should start
    /// normally rather than exiting and doing nothing at all.
    /// </summary>
    public bool TryHandOff(CommandLineRequest request)
    {
        try
        {
            using var client = new NamedPipeClientStream(
                ".", _pipeName, PipeDirection.Out, PipeOptions.None,
                TokenImpersonationLevel.Identification);

            client.Connect((int)ConnectTimeout.TotalMilliseconds);

            var payload = Encoding.UTF8.GetBytes(NavigationRequest.Format(request) + "\n");
            client.Write(payload, 0, payload.Length);
            client.Flush();
            return true;
        }
        catch (Exception ex) when (ex is TimeoutException or IOException or UnauthorizedAccessException
                                      or ObjectDisposedException or InvalidOperationException)
        {
            return false;
        }
    }

    private void Listen()
    {
        while (!_stopping.IsCancellationRequested)
        {
            try
            {
                using var server = CreateServer();
                server.WaitForConnection();

                if (_stopping.IsCancellationRequested) return;
                if (!IsOurOwnUser(server)) continue;

                if (ReadLine(server) is { } line &&
                    NavigationRequest.TryParse(line, out var request) &&
                    request.HasTargets)
                {
                    RequestReceived?.Invoke(request);
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                          or ObjectDisposedException or InvalidOperationException)
            {
                // One bad connection must not end the listener, or the app quietly stops answering
                // for the rest of the session.
                if (_stopping.IsCancellationRequested) return;
                Thread.Sleep(100);
            }
        }
    }

    private NamedPipeServerStream CreateServer()
    {
        var self = WindowsIdentity.GetCurrent().User
            ?? throw new InvalidOperationException("No user SID for the current process.");

        // Only this user may even open the pipe. Everything after this is defence in depth.
        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            self, PipeAccessRights.ReadWrite | PipeAccessRights.CreateNewInstance, AccessControlType.Allow));

        return NamedPipeServerStreamAcl.Create(
            _pipeName,
            PipeDirection.In,
            maxNumberOfServerInstances: 1,
            PipeTransmissionMode.Byte,
            PipeOptions.None,
            inBufferSize: 0,
            outBufferSize: 0,
            security);
    }

    /// <summary>The DACL should already have refused anyone else; this is the belt to its braces,
    /// and it costs nothing.</summary>
    private static bool IsOurOwnUser(NamedPipeServerStream server)
    {
        try
        {
            return server.GetImpersonationUserName()
                .Equals(WindowsIdentity.GetCurrent().Name, StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException)
        {
            return false;
        }
    }

    /// <summary>
    /// One bounded line. A peer that never sends a newline, or sends without end, gets cut off at
    /// <see cref="NavigationRequest.MaxLineLength"/> rather than being allowed to grow this buffer
    /// forever.
    /// </summary>
    private static string? ReadLine(Stream stream)
    {
        var buffer = new byte[1024];
        var line = new MemoryStream();

        while (line.Length < NavigationRequest.MaxLineLength)
        {
            var read = stream.Read(buffer, 0, buffer.Length);
            if (read <= 0) break;

            for (var i = 0; i < read; i++)
            {
                if (buffer[i] == (byte)'\n')
                    return Encoding.UTF8.GetString(line.ToArray());
                line.WriteByte(buffer[i]);
            }
        }

        // No newline arrived, but what did arrive may still be a whole request.
        return line.Length > 0 ? Encoding.UTF8.GetString(line.ToArray()) : null;
    }

    public void Dispose()
    {
        _stopping.Cancel();

        // The listener is parked in WaitForConnection, which only returns when something connects.
        // Connecting to ourselves is the tidy way to wake it; failing that it is a background
        // thread and process exit takes it.
        if (_listener is not null && IsFirst)
        {
            try
            {
                using var wake = new NamedPipeClientStream(".", _pipeName, PipeDirection.Out);
                wake.Connect(100);
            }
            catch (Exception ex) when (ex is TimeoutException or IOException
                                          or UnauthorizedAccessException or InvalidOperationException)
            {
            }
            _listener.Join(TimeSpan.FromSeconds(1));
        }

        _stopping.Dispose();
        try
        {
            if (IsFirst) _mutex.ReleaseMutex();
        }
        catch (ApplicationException)
        {
            // Not held — nothing to release.
        }
        _mutex.Dispose();
    }

    /// <summary>
    /// A stable, filename-safe key for the current user. The SID rather than the name: names can
    /// contain characters a pipe name cannot, and two domains can share one.
    /// </summary>
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
}
