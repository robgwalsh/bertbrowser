using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Changes;

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

    /// <summary>
    /// How long to look for a helper that is already running before concluding there is none.
    /// </summary>
    /// <remarks>
    /// Short, because a helper parked waiting for an app connects within milliseconds — this is a
    /// rendezvous, not a launch, and there is no prompt for anybody to read. It is a backstop
    /// rather than the answer: the presence lock is what actually decides whether one is there.
    /// </remarks>
    private static readonly TimeSpan AttachTimeout = TimeSpan.FromSeconds(2);

    /// <summary>How long a clean shutdown waits. See <see cref="Dispose"/> — it is a courtesy, not
    /// a guarantee.</summary>
    private static readonly TimeSpan ShutdownTimeout = TimeSpan.FromSeconds(2);

    private readonly IIndexHostLauncher _launcher;
    private readonly IIndexTransportFactory _transports;
    private readonly IIndexerPresence _presenceProbe;
    private readonly MftIndexState _state = new();
    private readonly object _gate = new();

    /// <summary>
    /// Serialises writes to the pipe. Until <see cref="ChangeLog"/> every line left from the one
    /// worker thread; its setter sends from whichever thread changed the setting, and two lines
    /// interleaved on the wire — say a <c>Record</c> through the middle of <c>Start</c> — would
    /// parse as nothing, and the helper would silently never index.
    /// </summary>
    private readonly object _sendGate = new();

    private CancellationTokenSource? _session;
    private Thread? _worker;
    private Stream? _stream;
    private string? _failure;
    private bool _canRetry;

    /// <summary>
    /// Whether asking for a helper could produce one.
    /// </summary>
    /// <remarks>
    /// Separate from <see cref="_canRetry"/> because they stopped meaning the same thing. Retrying
    /// answers a failure that is being reported; starting also covers the ordinary case where
    /// nothing has gone wrong and nobody has asked for an index yet — which has an empty status
    /// line, and where a "try again" would be answering a question nobody was asked.
    /// </remarks>
    private bool _canStart;
    private bool _disposed;
    private ChangeLogPolicy _changeLog;
    private IndexerPresence _presence = IndexerPresence.NotRunning;

    public MftIndexClient(IIndexHostLauncher launcher, IIndexTransportFactory transports,
        IIndexerPresence? presence = null)
    {
        _launcher = launcher;
        _transports = transports;
        _presenceProbe = presence ?? new NoIndexerPresence();
    }

    public event Action<string>? IndexRefreshed;
    public event Action? StatusChanged;

    public bool AnyIndexed => _state.AnyIndexed;

    public bool IsBuilding => _state.IsBuilding;

    public IReadOnlyCollection<string> BuildingDrives => _state.BuildingDrives;

    public IReadOnlyCollection<string> CompletedRoots => _state.CompletedRoots;

    public IndexerPresence Presence
    {
        get { lock (_gate) return _presence; }
    }

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

    /// <inheritdoc/>
    public bool CanStart
    {
        get { lock (_gate) return _canStart; }
    }

    /// <summary>
    /// Relayed to the helper the moment it is set, if one is up, and again after every Start —
    /// see <see cref="Converse"/>. The helper is never trusted to remember it across sessions.
    /// </summary>
    public ChangeLogPolicy ChangeLog
    {
        get { lock (_gate) return _changeLog; }
        set
        {
            Stream? stream;
            lock (_gate)
            {
                _changeLog = value;
                stream = _stream;
            }
            if (stream is not null) Send(stream, RecordMessage(value));
        }
    }

    private static IndexMessage RecordMessage(ChangeLogPolicy policy) =>
        new(IndexVerb.Record, policy.ToHours().ToString(System.Globalization.CultureInfo.InvariantCulture));

    public void Start() => Start(IndexStartMode.AttachOrLaunch);

    /// <summary>
    /// Looks for a helper, and — depending on <paramref name="mode"/> — starts one if there is none.
    /// </summary>
    /// <remarks>
    /// One entry point rather than a separate <c>Attach</c>, because the guard below is on a
    /// session being in flight: an attach followed by a start would find a worker already running
    /// and quietly do nothing.
    /// </remarks>
    public void Start(IndexStartMode mode)
    {
        Thread worker;
        lock (_gate)
        {
            if (_disposed || _worker is not null) return;
            var session = new CancellationTokenSource();
            _session = session;
            // Captured rather than re-read after the lock: a Dispose racing this would otherwise
            // null the field and leave a session that never starts.
            worker = new Thread(() => RunSession(mode, session.Token))
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
        Start(IndexStartMode.AttachOrLaunch);
    }

    private void RunSession(IndexStartMode mode, CancellationToken ct)
    {
        IIndexTransport? transport = null;
        try
        {
            if (!_transports.TryCreate(out transport, out var whyNot) || transport is null)
            {
                // Another copy of this app owns the endpoint. A prompt could not help, so this is
                // deliberately not retryable-by-prompting.
                Settle(IndexerPresence.NotRunning, Unavailable(whyNot), canStart: false);
                return;
            }

            // Attaching costs nothing and prompts nobody, so it comes before every other question —
            // including whether this account could elevate. A standard user who somehow has a
            // helper running should still get to use it.
            var attached = transport.Accept(null, AttachTimeout);
            if (attached is not null)
            {
                lock (_gate) _stream = attached;
                Converse(attached, null, ct);
                return;
            }

            if (_presenceProbe.IsRunning)
            {
                // A helper is there but would not talk to us. Starting another cannot work — its
                // own single-instance guard would refuse — so the prompt is withheld and the state
                // is reported as what it is. This is also the case where a change to the recording
                // policy cannot reach the helper, which the settings page has to be able to say.
                Settle(IndexerPresence.Running,
                    "Search index unavailable — the index helper is running but not responding.",
                    canStart: false);
                return;
            }

            if (mode == IndexStartMode.AttachOnly)
            {
                // Nothing is running and nobody asked for one. Say nothing in the status bar; the
                // banner is what speaks now, and two voices saying it is worse than one.
                Settle(IndexerPresence.NotRunning, failure: null, canStart: _launcher.CanElevate);
                return;
            }

            if (!_launcher.CanElevate)
            {
                // Deliberately without prompting: see IIndexHostLauncher.CanElevate.
                Fail("Search index off — this account is not an administrator.", canRetry: false);
                return;
            }

            var launch = _launcher.Launch();

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
            // The transport is emphatically not disposed here any more. It is the app's one
            // endpoint and has to stay listening between sessions, or a helper started after this
            // attempt would have nothing to connect to. The factory owns its lifetime.
            RetireSession();
        }
    }

    /// <summary>
    /// Lets go of the session this thread was running, so a later <see cref="Start"/> can begin a
    /// new one.
    /// </summary>
    /// <remarks>
    /// <b>Only if the fields still point at <em>this</em> thread.</b> <see cref="EndSession"/> may
    /// already have replaced them with a newer session's, and clearing those would leave a live
    /// worker nothing can reach and a <see cref="Start"/> that begins a second one beside it.
    /// Without this the fields kept whatever a failed attempt left behind, and the guard in
    /// <see cref="Start"/> then refused every later attempt — survivable while only
    /// <see cref="Retry"/> could restart, and not once a banner button calls <see cref="Start"/>.
    /// </remarks>
    private void RetireSession()
    {
        CancellationTokenSource? session = null;
        lock (_gate)
        {
            _stream = null;
            if (ReferenceEquals(_worker, Thread.CurrentThread))
            {
                session = _session;
                _worker = null;
                _session = null;
            }
        }

        // Disposing here is what stops a failed attempt leaking its source, since EndSession will
        // no longer find it. A concurrent EndSession may already hold the same reference, which is
        // why its Cancel tolerates a source this one has since disposed.
        try { session?.Dispose(); } catch (ObjectDisposedException) { }
    }

    private void Converse(Stream stream, int? launchedProcessId, CancellationToken ct)
    {
        var reader = new LineReader(stream, NavigationRequest.MaxLineLength);

        lock (_gate)
        {
            _presence = IndexerPresence.Running;
            _canRetry = false;
            _canStart = false;
            _failure = null;
        }
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
                        // A helper left over from a previous build of the app, still holding the
                        // endpoint. Mirroring state from something that means something else by it
                        // is the worse option — and since it will not exit on its own while it has
                        // an index to keep, ask it to.
                        Send(stream, new IndexMessage(IndexVerb.Shutdown));
                        Fail("Search index unavailable — restart to update the index helper.", canRetry: true);
                        return;
                    }
                    break;

                case IndexVerb.Ready when !started:
                    started = true;
                    Send(stream, new IndexMessage(IndexVerb.Start));
                    // Every session, even when it is the default: a retried helper is a fresh
                    // process whose own default is off, and the user's setting may not be.
                    Send(stream, RecordMessage(ChangeLog));
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

        // Only a helper this app launched can be waited on; an attached one has no handle here.
        if (launchedProcessId is { } processId)
            _launcher.WaitForExit(processId, ShutdownTimeout);
    }

    private void Send(Stream stream, IndexMessage message)
    {
        try
        {
            lock (_sendGate)
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
            // A failure worth retrying is a failure a start could answer.
            _canStart = canRetry;
            _presence = IndexerPresence.NotRunning;
        }
        StatusChanged?.Invoke();
    }

    /// <summary>
    /// Records that there is no helper, without calling it a failure.
    /// </summary>
    /// <remarks>
    /// <b>The difference from <see cref="Fail"/> is the empty status line, and it is the whole
    /// point.</b> "No helper is running because nobody has asked for one" is the ordinary state of
    /// a freshly launched app now, not an error — the banner is what offers to fix it, and a status
    /// bar saying the same thing at the same time reads as two problems instead of one offer.
    /// </remarks>
    private void Settle(IndexerPresence presence, string? failure, bool canStart)
    {
        _state.Clear();
        lock (_gate)
        {
            _failure = failure;
            // Only a reported failure may offer to be retried. Nothing running because nobody has
            // asked is not a failure, has no status line, and must not put a "try again" beside an
            // empty one.
            _canRetry = failure is not null && canStart;
            _canStart = canStart;
            _presence = presence;
        }
        StatusChanged?.Invoke();
    }

    private void Announce()
    {
        // A helper reporting progress is a working helper: nothing to retry, nothing to start.
        lock (_gate)
        {
            _failure = null;
            _canRetry = false;
            _canStart = false;
        }
        StatusChanged?.Invoke();
    }

    /// <summary>
    /// Asks the helper to stop, and waits briefly for it to.
    /// </summary>
    /// <remarks>
    /// The only way an app ends a helper now, and therefore the only thing behind the Stop button
    /// in Settings. <see cref="Dispose"/> deliberately does not do this.
    /// </remarks>
    public void Stop()
    {
        Stream? stream;
        lock (_gate) stream = _stream;
        if (stream is not null) Send(stream, new IndexMessage(IndexVerb.Shutdown));

        EndSession();
        Settle(IndexerPresence.NotRunning, failure: null, canStart: _launcher.CanElevate);
    }

    /// <summary>
    /// Lets go of the helper without ending it.
    /// </summary>
    /// <remarks>
    /// <b>This used to send <see cref="IndexVerb.Shutdown"/>, and that line was what killed the
    /// helper every time the app closed.</b> The helper is meant to outlive the app now — that is
    /// the whole feature — so closing a window hangs up and nothing more. It keeps indexing, keeps
    /// tailing the journal, and the next app to start attaches to it without a prompt.
    /// <para>
    /// Nothing here could force the point anyway: a medium-integrity process may not open a
    /// high-integrity one for <c>PROCESS_TERMINATE</c>, nor put it in a job object. What ends a
    /// helper is <see cref="Stop"/>, signing out, or its own self-checks.
    /// </para>
    /// </remarks>
    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
        }

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

        // The worker may have retired and disposed this source already — see RetireSession.
        try { session?.Cancel(); } catch (ObjectDisposedException) { }
        // The worker is parked in a blocking read; closing the stream is what returns it.
        try { stream?.Dispose(); } catch (Exception ex) when (ex is IOException or ObjectDisposedException) { }
        worker?.Join(ShutdownTimeout);
        session?.Dispose();
    }
}
