using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;

namespace BertBrowser.Core.Services.Mft;

/// <summary>How a session ended, and therefore whether the process should carry on.</summary>
public enum IndexSessionEnd
{
    /// <summary>The app went away. Another one may arrive; the index keeps running.</summary>
    PeerGone,

    /// <summary>An app asked this process to stop, or it cannot continue.</summary>
    Stop,
}

/// <summary>
/// The elevated process's half: runs a real <see cref="IMftIndexService"/> and reports it down the
/// pipe, taking five verbs and nothing else.
/// </summary>
/// <remarks>
/// <para>
/// What this will accept is the security surface of the index design. It is
/// <see cref="IndexVerb.Hello"/>, <see cref="IndexVerb.Start"/>, <see cref="IndexVerb.Shutdown"/>,
/// <see cref="IndexVerb.Ping"/> and <see cref="IndexVerb.Record"/> — none of which names a file, a
/// folder or a program. <see cref="IndexVerb.Record"/> carries one integer from a fixed menu and
/// nothing else. Adding a verb that takes a path would undo the point of the split, however
/// convenient it looked at the time.
/// </para>
/// <para>
/// <b>That rule matters more now than it did, not less.</b> This process used to die with the app
/// that launched it; it now outlives every one of them and can be started at sign-in by a scheduled
/// task, so there may be no supervising parent at all. An always-on administrator-token process
/// that could be aimed at a path is a considerably worse thing than one that could.
/// </para>
/// <para>
/// <b>One host, many sessions.</b> Losing the pipe ends the <em>session</em>, not the process: the
/// index and its USN tail keep running and the next app to start attaches to them. What ends the
/// process is <see cref="IndexVerb.Shutdown"/>, signing out, or the self-checks in the caller.
/// </para>
/// <para>
/// Because an app can attach long after volumes finished building, every session opens by
/// <em>replaying</em> what is already known — <see cref="IMftIndexService.IndexRefreshed"/> fired
/// for those volumes before this app existed and will not fire again.
/// <see cref="IndexVerb.Ready"/> therefore means "I have told you everything I know", and is sent
/// last.
/// </para>
/// </remarks>
public sealed class MftIndexHost
{
    private readonly IMftIndexService _index;
    private readonly object _writeGate = new();
    private readonly HashSet<string> _reportedBuilding = new(StringComparer.Ordinal);

    /// <summary>
    /// The stream of the session in progress, or null between sessions.
    /// </summary>
    /// <remarks>
    /// A volume thread's status change can fire at any moment, including while nothing is attached.
    /// Dropping those is safe precisely because the next session replays the whole picture.
    /// </remarks>
    private Stream? _stream;

    /// <summary>
    /// Whether indexing has been asked for. An instance field, not a local in <see cref="Run"/>: a
    /// second app's <see cref="IndexVerb.Start"/> must not start a second set of volume threads.
    /// <see cref="MftIndexService.Start"/> is idempotent on its own too — both, because this one is
    /// the local guarantee and losing it silently would be hard to notice.
    /// </summary>
    private bool _started;

    public MftIndexHost(IMftIndexService index) => _index = index;

    /// <summary>
    /// Talks over one connection until the app goes away or asks this process to stop.
    /// </summary>
    public IndexSessionEnd Run(Stream stream, CancellationToken ct = default)
    {
        var reader = new LineReader(stream, NavigationRequest.MaxLineLength);

        lock (_writeGate)
        {
            _stream = stream;
            // Forget what the previous app was told; this one has heard nothing.
            _reportedBuilding.Clear();
        }

        _index.IndexRefreshed += OnIndexRefreshed;
        _index.StatusChanged += OnStatusChanged;
        try
        {
            Send(new IndexMessage(IndexVerb.Hello, IndexProtocol.ProtocolVersion.ToString()));
            Replay();
            Send(new IndexMessage(IndexVerb.Ready));

            while (!ct.IsCancellationRequested)
            {
                var line = reader.ReadLine();
                if (line is null) return IndexSessionEnd.PeerGone; // The app is gone.

                if (!IndexProtocol.TryParse(line, out var message)) continue;

                switch (message.Verb)
                {
                    case IndexVerb.Hello:
                        if (IndexProtocol.VersionOf(message) != IndexProtocol.ProtocolVersion)
                        {
                            Send(new IndexMessage(IndexVerb.Fatal, "Version mismatch."));
                            return IndexSessionEnd.Stop;
                        }
                        break;

                    case IndexVerb.Start when !_started:
                        _started = true;
                        StartIndexing();
                        break;

                    case IndexVerb.Ping:
                        Send(new IndexMessage(IndexVerb.Pong));
                        break;

                    case IndexVerb.Record:
                        // TryParse admitted only the menu, so this cannot throw.
                        _index.ChangeLog = Services.Changes.ChangeLogPolicy.FromHours(
                            int.Parse(message.Argument, System.Globalization.CultureInfo.InvariantCulture));
                        break;

                    case IndexVerb.Shutdown:
                        return IndexSessionEnd.Stop;

                    // Anything else is the app speaking the helper's half of the protocol back at
                    // it. Ignored rather than answered.
                }
            }

            return IndexSessionEnd.Stop;
        }
        finally
        {
            _index.IndexRefreshed -= OnIndexRefreshed;
            _index.StatusChanged -= OnStatusChanged;
            lock (_writeGate) _stream = null;
        }
    }

    /// <summary>
    /// Brings a freshly attached app up to date on everything that happened before it existed.
    /// </summary>
    /// <remarks>
    /// The building set needs no code of its own: <see cref="OnStatusChanged"/> already diffs the
    /// service's drives against <see cref="_reportedBuilding"/>, which the session just cleared, so
    /// one call announces all of them and re-seeds the set. A second code path for the same job
    /// would be one more thing that could word it differently.
    /// </remarks>
    private void Replay()
    {
        OnStatusChanged();

        foreach (var root in _index.CompletedRoots)
        {
            if (IndexProtocol.IsAcceptableRootKey(root))
                Send(new IndexMessage(IndexVerb.Complete, root));
        }
    }

    private void StartIndexing()
    {
        try
        {
            _index.Start();
        }
        catch (Exception ex)
        {
            // The app cannot see this process's exceptions, so an indexer that cannot run has to
            // say so on the wire or it looks exactly like one that is merely slow.
            Send(new IndexMessage(IndexVerb.Fatal, Summarize(ex)));
            throw;
        }
    }

    private void OnIndexRefreshed(string rootKey)
    {
        if (IndexProtocol.IsAcceptableRootKey(rootKey))
            Send(new IndexMessage(IndexVerb.Complete, rootKey));
    }

    /// <summary>
    /// Relays which drives are building, one message per change.
    /// </summary>
    /// <remarks>
    /// The drives are sent, never the status <em>text</em>. The client holds the same
    /// <see cref="MftIndexState"/> this side does and formats the line with the same function, so
    /// there is one place that decides how "Indexing C:, D:…" is worded and no way for the two
    /// processes to word it differently. It also means the client can answer
    /// <see cref="IMftIndexService.IsBuilding"/> exactly rather than by inspecting a string.
    /// </remarks>
    private void OnStatusChanged()
    {
        lock (_writeGate)
        {
            // Anything the protocol would not carry is dropped here rather than tracked and then
            // silently never sent, which would leave this side believing it had reported it.
            var current = _index.BuildingDrives.Where(IndexProtocol.IsAcceptableDrive).ToHashSet(StringComparer.Ordinal);

            foreach (var drive in current)
            {
                if (_reportedBuilding.Add(drive))
                    Send(new IndexMessage(IndexVerb.Building, drive));
            }

            foreach (var drive in _reportedBuilding.Except(current).ToList())
            {
                _reportedBuilding.Remove(drive);
                Send(new IndexMessage(IndexVerb.Idle, drive));
            }
        }
    }

    private void Send(IndexMessage message)
    {
        try
        {
            lock (_writeGate)
            {
                // Between sessions there is nobody to tell, and the next session replays anyway.
                if (_stream is not { } stream) return;
                LineChannel.WriteLine(stream, IndexProtocol.Format(message));
            }
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException)
        {
            // The app has gone. The read loop is about to see the same thing and return.
        }
    }

    private static string Truncate(string text) =>
        text.Length <= IndexProtocol.MaxStatusLength ? text : text[..IndexProtocol.MaxStatusLength];

    /// <summary>One line, no control characters — whatever the exception's own message contains.</summary>
    private static string Summarize(Exception ex)
    {
        var text = ex.Message.ReplaceLineEndings(" ");
        var clean = new string(text.Select(c => char.IsControl(c) ? ' ' : c).ToArray()).Trim();
        return clean.Length == 0 ? "The search index could not start." : Truncate(clean);
    }
}
