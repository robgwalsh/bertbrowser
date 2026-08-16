using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// The app's half of the split: launches the elevated indexer, mirrors what it reports, and answers
/// as if the index were running here.
/// </summary>
/// <remarks>
/// <para>
/// Everything <see cref="IMftIndexService"/> exposes is answered from a local
/// <see cref="MftIndexState"/> fed by pushes from the helper, so nothing the UI or the search router
/// asks ever waits on a round trip. That is the whole reason the protocol pushes state rather than
/// answering questions.
/// </para>
/// <para>
/// <b>Nothing here retries by itself.</b> A retry raises a UAC prompt, and a prompt nobody asked for
/// that reappears on a timer is worse than having no index — so a failure is a state with
/// <see cref="CanRetry"/> set, and the user clicks.
/// </para>
/// <para>
/// Losing the helper is not an error condition to recover from silently: the mirrored state is
/// <em>cleared</em>, because claiming a volume is indexed when the process that indexed it has gone
/// would route searches to a database nothing is keeping current.
/// </para>
/// </remarks>
public sealed class MftIndexClient : IMftIndexService
{
    /// <summary>Long enough for an elevated process to start behind a UAC prompt the user has to
    /// read, short enough that a helper which never arrives does not hold the status bar forever.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(60);

    /// <summary>How long a clean shutdown waits. See <see cref="Dispose"/> — it is a courtesy, not
    /// a guarantee.</summary>
    private static readonly TimeSpan ShutdownTimeout = TimeSpan.FromSeconds(2);

    private readonly IIndexHostLauncher _launcher;
    private readonly IIndexTransportFactory _transports;
    private readonly MftIndexState _state = new();
    private readonly object _gate = new();

    private CancellationTokenSource? _session;
    private Thread? _worker;
    private Stream? _stream;
    private string? _failure;
    private bool _canRetry;
    private bool _disposed;

    public MftIndexClient(IIndexHostLauncher launcher, IIndexTransportFactory transports)
    {
        _launcher = launcher;
        _transports = transports;
    }

    public event Action<string>? IndexRefreshed;
    public event Action? StatusChanged;

    public bool AnyIndexed => _state.AnyIndexed;

    public bool IsBuilding => _state.IsBuilding;

    public IReadOnlyCollection<string> BuildingDrives => _state.BuildingDrives;

    public bool IsIndexed(string pathKey) => _state.IsIndexed(pathKey);

    public string StatusText
    {
        get
        {
            lock (_gate)
            {
                // The wording is MftIndexState's, the same function the in-process indexer uses, so
                // the two can never phrase the same state differently.
                return _failure ?? _state.FormatStatus();
            }
        }
    }

    public bool CanRetry
    {
        get { lock (_gate) return _canRetry; }
    }

    public void Start()
    {
        Thread worker;
        lock (_gate)
        {
            if (_disposed || _worker is not null) return;
            var session = new CancellationTokenSource();
            _session = session;
            // Captured rather than re-read after the lock: a Dispose racing this would otherwise
            // null the field and leave a session that never starts.
            worker = new Thread(() => RunSession(session.Token))
            {
                IsBackground = true,
                Name = "bertbrowser index client",
            };
            _worker = worker;
        }
        worker.Start();
    }

    public void Retry()
    {
        lock (_gate)
        {
            if (_disposed || !_canRetry) return;
            _canRetry = false;
            _failure = null;
        }

        EndSession();
        Start();
    }

    private void RunSession(CancellationToken ct)
    {
        if (!_launcher.CanElevate)
        {
            // Deliberately without prompting: see IIndexHostLauncher.CanElevate.
            Fail("Search index off — this account is not an administrator.", canRetry: false);
            return;
        }

        IIndexTransport? transport = null;
        try
        {
            transport = _transports.Create();
            var launch = _launcher.Launch(transport.Endpoint, Environment.ProcessId);

            switch (launch.Outcome)
            {
                case IndexHostLaunch.Declined:
                    Fail("Search index off — permission declined.", canRetry: true);
                    return;
                case IndexHostLaunch.NotAdministrator:
                    Fail("Search index off — this account is not an administrator.", canRetry: false);
                    return;
                case IndexHostLaunch.Failed:
                    Fail(Unavailable(launch.Detail), canRetry: true);
                    return;
            }

            var stream = transport.Accept(launch.ProcessId, ConnectTimeout);
            if (stream is null)
            {
                Fail("Search index unavailable.", canRetry: true);
                return;
            }

            lock (_gate) _stream = stream;
            Converse(stream, launch.ProcessId, ct);
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException
                                      or InvalidOperationException or UnauthorizedAccessException)
        {
            if (!ct.IsCancellationRequested)
                Fail("Search index stopped.", canRetry: true);
        }
        finally
        {
            transport?.Dispose();
            lock (_gate) _stream = null;
        }
    }

    private void Converse(Stream stream, int processId, CancellationToken ct)
    {
        var reader = new LineReader(stream, NavigationRequest.MaxLineLength);

        Send(stream, new IndexMessage(IndexVerb.Hello, IndexProtocol.ProtocolVersion.ToString()));

        var started = false;
        while (!ct.IsCancellationRequested)
        {
            var line = reader.ReadLine();
            if (line is null) break; // The helper is gone. See the class remarks.

            // One malformed message must not end the session.
            if (!IndexProtocol.TryParse(line, out var message)) continue;

            switch (message.Verb)
            {
                case IndexVerb.Hello:
                    if (IndexProtocol.VersionOf(message) != IndexProtocol.ProtocolVersion)
                    {
                        // A half-applied update left a stale executable behind. Mirroring state
                        // from something that means something else by it is the worse option.
                        Fail("Search index unavailable.", canRetry: true);
                        return;
                    }
                    break;

                case IndexVerb.Ready when !started:
                    started = true;
                    Send(stream, new IndexMessage(IndexVerb.Start));
                    break;

                case IndexVerb.Building:
                    _state.MarkBuilding(message.Argument);
                    Announce();
                    break;

                case IndexVerb.Idle:
                    _state.ClearBuilding(message.Argument);
                    Announce();
                    break;

                case IndexVerb.Complete:
                    _state.MarkComplete(message.Argument);
                    Announce();
                    IndexRefreshed?.Invoke(message.Argument);
                    break;

                case IndexVerb.Fatal:
                    Fail(message.Argument, canRetry: true);
                    return;

                case IndexVerb.Ping:
                    Send(stream, new IndexMessage(IndexVerb.Pong));
                    break;
            }
        }

        if (!ct.IsCancellationRequested)
            Fail("Search index stopped.", canRetry: true);

        _launcher.WaitForExit(processId, ShutdownTimeout);
    }

    private static void Send(Stream stream, IndexMessage message)
    {
        try
        {
            LineChannel.WriteLine(stream, IndexProtocol.Format(message));
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException)
        {
            // The reader will see the same break and end the session.
        }
    }

    /// <summary>
    /// A launch failure the user did not choose, said as specifically as the launcher can manage.
    /// </summary>
    /// <remarks>
    /// The status bar is the only place this can be said — there is no log — and "unavailable" on
    /// its own sends the reader to a debugger. A missing helper beside the executable and a UAC
    /// subsystem that refused are the same sentence otherwise, and they are not the same problem.
    /// The details are written as lowercase fragments so they compose into this one line — except
    /// the one that is an OS message quoted verbatim, which is why the terminator is trimmed rather
    /// than assumed absent.
    /// </remarks>
    private static string Unavailable(string detail)
    {
        detail = detail.Trim().TrimEnd('.');
        return detail.Length == 0
            ? "Search index unavailable."
            : $"Search index unavailable — {detail}.";
    }

    private void Fail(string status, bool canRetry)
    {
        _state.Clear();
        lock (_gate)
        {
            _failure = status;
            _canRetry = canRetry;
        }
        StatusChanged?.Invoke();
    }

    private void Announce()
    {
        lock (_gate) _failure = null;
        StatusChanged?.Invoke();
    }

    /// <summary>
    /// Asks the helper to stop, then lets go.
    /// </summary>
    /// <remarks>
    /// <b>This app cannot kill its own helper.</b> A medium-integrity process may not open a
    /// high-integrity one for <c>PROCESS_TERMINATE</c>, nor put it in a job object. What actually
    /// guarantees the helper dies is the pipe breaking — the kernel does that when this process
    /// ends, however it ends — backed by the helper's own watchdog on this process's handle. The
    /// <see cref="IndexVerb.Shutdown"/> below and the wait after it are the tidy path, not the
    /// guarantee.
    /// </remarks>
    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
        }

        Stream? stream;
        lock (_gate) stream = _stream;
        if (stream is not null) Send(stream, new IndexMessage(IndexVerb.Shutdown));

        EndSession();
    }

    private void EndSession()
    {
        CancellationTokenSource? session;
        Thread? worker;
        Stream? stream;
        lock (_gate)
        {
            session = _session;
            worker = _worker;
            stream = _stream;
            _session = null;
            _worker = null;
        }

        session?.Cancel();
        // The worker is parked in a blocking read; closing the stream is what returns it.
        try { stream?.Dispose(); } catch (Exception ex) when (ex is IOException or ObjectDisposedException) { }
        worker?.Join(ShutdownTimeout);
        session?.Dispose();
    }
}
