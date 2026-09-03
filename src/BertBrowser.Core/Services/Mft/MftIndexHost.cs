using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;

namespace BertBrowser.Core.Services.Mft;

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
/// convenient it looked at the time, and the arrival of a second elevated helper changes nothing
/// about that: this process is long-lived and starts itself at launch, which is precisely why it
/// may not be told where to point. See <c>IndexProtocol</c> for the argument in full.
/// </para>
/// <para>
/// <b>Losing the pipe is how this process learns to exit.</b> The kernel breaks it when the app
/// ends, crash included, so a read returning null is the primary shutdown signal and needs no
/// timer. The caller adds a watchdog on the parent's handle for the exotic case where a duplicated
/// handle keeps the pipe alive; the explicit <see cref="IndexVerb.Shutdown"/> is just the tidy path.
/// </para>
/// </remarks>
public sealed class MftIndexHost
{
    private readonly IMftIndexService _index;
    private readonly Stream _stream;
    private readonly object _writeGate = new();
    private readonly HashSet<string> _reportedBuilding = new(StringComparer.Ordinal);

    public MftIndexHost(IMftIndexService index, Stream stream)
    {
        _index = index;
        _stream = stream;
    }

    /// <summary>
    /// Talks until the app goes away, the app says stop, or indexing cannot continue. Returns when
    /// the process should exit.
    /// </summary>
    public void Run(CancellationToken ct = default)
    {
        var reader = new LineReader(_stream, NavigationRequest.MaxLineLength);
        var started = false;

        _index.IndexRefreshed += OnIndexRefreshed;
        _index.StatusChanged += OnStatusChanged;
        try
        {
            Send(new IndexMessage(IndexVerb.Hello, IndexProtocol.ProtocolVersion.ToString()));
            Send(new IndexMessage(IndexVerb.Ready));

            while (!ct.IsCancellationRequested)
            {
                var line = reader.ReadLine();
                if (line is null) return; // The app is gone.

                if (!IndexProtocol.TryParse(line, out var message)) continue;

                switch (message.Verb)
                {
                    case IndexVerb.Hello:
                        if (IndexProtocol.VersionOf(message) != IndexProtocol.ProtocolVersion)
                        {
                            Send(new IndexMessage(IndexVerb.Fatal, "Version mismatch."));
                            return;
                        }
                        break;

                    case IndexVerb.Start when !started:
                        started = true;
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
                        return;

                    // Anything else is the app speaking the helper's half of the protocol back at
                    // it. Ignored rather than answered.
                }
            }
        }
        finally
        {
            _index.IndexRefreshed -= OnIndexRefreshed;
            _index.StatusChanged -= OnStatusChanged;
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
                LineChannel.WriteLine(_stream, IndexProtocol.Format(message));
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
