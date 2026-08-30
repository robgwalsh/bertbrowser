using System.IO;
using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Threading;
using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;

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
/// Both peers are ordinary medium-integrity processes belonging to the same user, so the pipe needs
/// no mandatory-label work: a DACL admitting only the current user's SID is exactly right. The
/// client's identity is re-checked after connecting anyway.
/// </para>
/// <para>
/// Its framing and identity comparison come from <c>Core/Ipc</c>, shared with the index-helper pipe.
/// One thing here is <em>not</em> shared and must not be copied from: this pipe is
/// <see cref="PipeDirection.In"/> and the server never writes a byte, which is the only reason
/// zero-size buffers are safe. A duplex pipe with no buffers deadlocks the moment both ends speak.
/// </para>
/// <para>
/// Nothing here is a security boundary against the user themselves — it cannot be. What it does
/// bound is the <em>protocol</em>: one line, one request, "navigate to this path", validated by
/// <see cref="NavigationRequest.IsAcceptablePath"/>. Nothing on the wire can become a launch, a
/// file that gets written, or anything but a directory listing.
/// </para>
/// <para>
/// <b>A second account signed in to the same machine is a different question, and the endpoint name
/// is the answer to it</b> — see <see cref="InstanceEndpoint"/> for why a predictable name was one
/// another user could claim, and what that cost. The copy that owns the name
/// <see cref="Publish">publishes</see> it to <see cref="AppPaths.DataDir"/> for the next launch to
/// read; the profile's own permissions are what keep that file to one account, and the mutex — which
/// is <c>Local\</c>, and so per-session and unsquattable — remains what decides who is first.
/// </para>
/// </remarks>
public sealed class SingleInstance : IDisposable
{
    /// <summary>Long enough to cross a busy machine, short enough that a wedged first instance does
    /// not stall the second one's startup.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromMilliseconds(500);

    /// <summary>Where the running copy leaves its endpoint name for the next launch to find.</summary>
    private static string EndpointPath => Path.Combine(AppPaths.DataDir, "instance.pipe");

    /// <summary>Null for a later copy that found nothing published — there is nobody to hand to.</summary>
    private readonly string? _pipeName;
    private readonly Mutex _mutex;
    private readonly CancellationTokenSource _stopping = new();
    private Thread? _listener;

    private SingleInstance(string? pipeName, Mutex mutex, bool isFirst)
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
        // privileges we should not be relying on. The mutex is what decides who is first — a name in
        // this namespace cannot be claimed from another session, which is exactly the property the
        // pipe namespace lacks and why the endpoint below is named the way it is.
        var mutex = new Mutex(initiallyOwned: true, $"Local\\BertBrowser.{key}", out var isFirst);

        // The first copy invents its endpoint and publishes it once it is listening; a later one can
        // only be told, and starts normally if it was not.
        var pipeName = isFirst ? InstanceEndpoint.Name(key, Nonce()) : Published(key);
        return new SingleInstance(pipeName, mutex, isFirst);
    }

    /// <summary>Starts listening, then publishes where. Only the first instance should call this.</summary>
    public void StartListening()
    {
        if (!IsFirst || _pipeName is null) return;

        _listener = new Thread(Listen)
        {
            IsBackground = true,
            Name = "BertBrowser single-instance listener",
        };
        _listener.Start();

        // After the thread, so the window between "a launch can find the name" and "something is
        // answering to it" is as small as it can be made. A launch that lands inside it fails to
        // connect and starts normally, which is the same fallback a mid-shutdown first copy gets.
        Publish(_pipeName);
    }

    /// <summary>
    /// Hands <paramref name="request"/> to the copy already running. False when it could not be
    /// reached — the first instance may be mid-shutdown — in which case the caller should start
    /// normally rather than exiting and doing nothing at all.
    /// </summary>
    public bool TryHandOff(CommandLineRequest request)
    {
        if (_pipeName is null) return false;

        try
        {
            using var client = new NamedPipeClientStream(
                ".", _pipeName, PipeDirection.Out, PipeOptions.None,
                TokenImpersonationLevel.Identification);

            client.Connect((int)ConnectTimeout.TotalMilliseconds);

            // Before the write, not after: this process holds the foreground rights (the shell just
            // started it from a double-click) and the copy doing the work does not. Without the
            // hand-over its window opens the folder behind whatever the user was looking at.
            BertBrowser.App.Interop.ForegroundWindow.GrantTo(client.SafePipeHandle);

            LineChannel.WriteLine(client, NavigationRequest.Format(request));
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

                if (LineChannel.ReadLine(server, NavigationRequest.MaxLineLength) is { } line &&
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

    // --- Endpoint discovery ---

    /// <summary>
    /// Records the endpoint for the next launch to find. Best-effort: a name that cannot be written
    /// costs the hand-off and nothing else, and the next launch starts its own copy.
    /// </summary>
    /// <remarks>
    /// Deliberately does <em>not</em> create the directory. <c>AppPaths.MigrateLegacyData</c> moves
    /// pre-1.0 data only when the data directory does not yet exist, and it runs before this does —
    /// so a <c>CreateDirectory</c> here would be the thing that silently retired that migration.
    /// </remarks>
    private static void Publish(string pipeName)
    {
        try
        {
            File.WriteAllText(EndpointPath, pipeName);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or DirectoryNotFoundException)
        {
        }
    }

    /// <summary>
    /// The endpoint a running copy left behind, or null. A file left by a crash names a pipe nobody
    /// is answering, which the connect attempt discovers and treats as "no first instance".
    /// </summary>
    private static string? Published(string key)
    {
        try
        {
            if (!File.Exists(EndpointPath)) return null;
            var name = File.ReadAllText(EndpointPath).Trim();
            return InstanceEndpoint.IsAcceptable(name, key) ? name : null;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or DirectoryNotFoundException or ArgumentException)
        {
            return null;
        }
    }

    private static void Unpublish()
    {
        try
        {
            if (File.Exists(EndpointPath)) File.Delete(EndpointPath);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or DirectoryNotFoundException)
        {
        }
    }

    private static string Nonce() =>
        Convert.ToHexString(RandomNumberGenerator.GetBytes(InstanceEndpoint.NonceLength / 2));

    private NamedPipeServerStream CreateServer()
    {
        // Only the first instance listens, and only after Claim gave it a name of its own.
        var pipeName = _pipeName
            ?? throw new InvalidOperationException("Listening without an endpoint to listen on.");

        var self = WindowsIdentity.GetCurrent().User
            ?? throw new InvalidOperationException("No user SID for the current process.");

        // Only this user may even open the pipe. Everything after this is defence in depth.
        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            self, PipeAccessRights.ReadWrite | PipeAccessRights.CreateNewInstance, AccessControlType.Allow));

        return NamedPipeServerStreamAcl.Create(
            pipeName,
            PipeDirection.In,
            maxNumberOfServerInstances: 1,
            PipeTransmissionMode.Byte,
            PipeOptions.None,
            inBufferSize: 0,
            outBufferSize: 0,
            security);
    }

    /// <summary>
    /// The DACL should already have refused anyone else; this is the belt to its braces.
    /// See <see cref="PipeIdentity"/> for why the two names have to be compared the way they are.
    /// </summary>
    private static bool IsOurOwnUser(NamedPipeServerStream server)
    {
        try
        {
            return PipeIdentity.SameAccount(
                server.GetImpersonationUserName(), WindowsIdentity.GetCurrent().Name);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException)
        {
            // Indeterminate. The DACL is the real gate, but an unreadable client identity is
            // anomalous enough to refuse rather than wave through.
            return false;
        }
    }

    public void Dispose()
    {
        _stopping.Cancel();

        // Before waking the listener: from here on there is no endpoint to hand off to, and a launch
        // that reads a name we are about to stop answering has to wait out a connect timeout to find
        // that out.
        if (IsFirst) Unpublish();

        // The listener is parked in WaitForConnection, which only returns when something connects.
        // Connecting to ourselves is the tidy way to wake it; failing that it is a background
        // thread and process exit takes it.
        if (_listener is not null && IsFirst && _pipeName is not null)
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
