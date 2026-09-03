using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Core.Tests;

/// <summary>
/// A blocking in-memory stream pair, so both halves of the index protocol can be driven without a
/// real pipe — and therefore without a real elevation prompt, which no test can ever answer.
/// </summary>
internal static class DuplexPair
{
    public static (Stream Left, Stream Right) Create()
    {
        var leftToRight = new BlockingStreamBuffer();
        var rightToLeft = new BlockingStreamBuffer();
        return (new DuplexStream(rightToLeft, leftToRight), new DuplexStream(leftToRight, rightToLeft));
    }
}

/// <summary>One direction: writes append, reads block until there is something or it is closed.</summary>
internal sealed class BlockingStreamBuffer
{
    private readonly Queue<byte> _bytes = new();
    private readonly object _gate = new();
    private bool _closed;

    public void Write(byte[] buffer, int offset, int count)
    {
        lock (_gate)
        {
            if (_closed) throw new IOException("The pipe has been closed.");
            for (var i = 0; i < count; i++) _bytes.Enqueue(buffer[offset + i]);
            Monitor.PulseAll(_gate);
        }
    }

    public int Read(byte[] buffer, int offset, int count)
    {
        lock (_gate)
        {
            while (_bytes.Count == 0 && !_closed) Monitor.Wait(_gate);

            var read = 0;
            while (read < count && _bytes.Count > 0) buffer[offset + read++] = _bytes.Dequeue();
            return read; // 0 only once closed and drained, which is end-of-stream.
        }
    }

    public void Close()
    {
        lock (_gate)
        {
            _closed = true;
            Monitor.PulseAll(_gate);
        }
    }
}

internal sealed class DuplexStream : Stream
{
    private readonly BlockingStreamBuffer _in;
    private readonly BlockingStreamBuffer _out;

    public DuplexStream(BlockingStreamBuffer input, BlockingStreamBuffer output)
    {
        _in = input;
        _out = output;
    }

    public override bool CanRead => true;
    public override bool CanSeek => false;
    public override bool CanWrite => true;
    public override long Length => throw new NotSupportedException();
    public override long Position { get => 0; set => throw new NotSupportedException(); }
    public override void Flush() { }
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override int Read(byte[] buffer, int offset, int count) => _in.Read(buffer, offset, count);
    public override void Write(byte[] buffer, int offset, int count) => _out.Write(buffer, offset, count);

    /// <summary>Closing both directions is what a broken pipe looks like to the other end.</summary>
    protected override void Dispose(bool disposing)
    {
        _in.Close();
        _out.Close();
        base.Dispose(disposing);
    }
}

/// <summary>Reads and writes protocol messages on the far end of a <see cref="DuplexPair"/>.</summary>
internal sealed class ProtocolPeer
{
    private readonly Stream _stream;
    private readonly LineReader _reader;

    public ProtocolPeer(Stream stream)
    {
        _stream = stream;
        _reader = new LineReader(stream, 64 * 1024);
    }

    public void Send(IndexVerb verb, string argument = "") =>
        LineChannel.WriteLine(_stream, IndexProtocol.Format(new IndexMessage(verb, argument)));

    /// <summary>Sends a line the protocol will refuse, to prove a session survives one.</summary>
    public void SendRaw(string line) => LineChannel.WriteLine(_stream, line);

    /// <summary>The next parseable message, skipping anything malformed. Null at end of stream.</summary>
    public IndexMessage? Receive()
    {
        while (true)
        {
            var line = _reader.ReadLine();
            if (line is null) return null;
            if (IndexProtocol.TryParse(line, out var message)) return message;
        }
    }

    /// <summary>The next message with one of <paramref name="verbs"/>, skipping the others.</summary>
    public IndexMessage? ReceiveOneOf(params IndexVerb[] verbs)
    {
        while (Receive() is { } message)
        {
            if (verbs.Contains(message.Verb)) return message;
        }
        return null;
    }

    public void Close() => _stream.Dispose();
}

/// <summary>An <see cref="IIndexHostLauncher"/> that starts nothing and reports what it was told to.</summary>
internal sealed class FakeIndexHostLauncher : IIndexHostLauncher
{
    private readonly IndexHostLaunchResult _result;
    private readonly Action<string>? _onLaunch;

    public FakeIndexHostLauncher(IndexHostLaunchResult result, Action<string>? onLaunch = null)
    {
        _result = result;
        _onLaunch = onLaunch;
    }

    public int Launches { get; private set; }
    public bool CanElevate { get; init; } = true;

    public IndexHostLaunchResult Launch(string pipeName, int parentProcessId)
    {
        Launches++;
        _onLaunch?.Invoke(pipeName);
        return _result;
    }

    public void WaitForExit(int processId, TimeSpan timeout) { }
}

/// <summary>Hands the client one end of an already-connected pair.</summary>
internal sealed class FakeIndexTransportFactory : IIndexTransportFactory
{
    private readonly Func<Stream?> _accept;

    public FakeIndexTransportFactory(Func<Stream?> accept) => _accept = accept;

    public int Created { get; private set; }

    public IIndexTransport Create()
    {
        Created++;
        return new FakeIndexTransport(_accept);
    }

    private sealed class FakeIndexTransport : IIndexTransport
    {
        private readonly Func<Stream?> _accept;
        public FakeIndexTransport(Func<Stream?> accept) => _accept = accept;
        public string Endpoint => "BertBrowser.Index.Test";
        public Stream? Accept(int processId, TimeSpan timeout) => _accept();
        public void Dispose() { }
    }
}

/// <summary>An index service a test drives by hand, standing in for a real volume walk.</summary>
internal sealed class ControllableIndexService : IMftIndexService
{
    private readonly MftIndexState _state = new();

    public int Starts { get; private set; }
    public Exception? StartThrows { get; init; }

    public event Action<string>? IndexRefreshed;
    public event Action? StatusChanged;

    public bool AnyIndexed => _state.AnyIndexed;
    public bool IsBuilding => _state.IsBuilding;
    public IReadOnlyCollection<string> BuildingDrives => _state.BuildingDrives;
    public bool IsIndexed(string pathKey) => _state.IsIndexed(pathKey);
    public string StatusText => _state.FormatStatus();
    public bool CanRetry => false;
    public void Retry() { }
    public BertBrowser.Core.Services.Changes.ChangeLogPolicy ChangeLog { get; set; }

    public void Start()
    {
        Starts++;
        if (StartThrows is not null) throw StartThrows;
    }

    public void BeginBuilding(string drive)
    {
        _state.MarkBuilding(drive);
        StatusChanged?.Invoke();
    }

    public void FinishBuilding(string drive, string rootKey)
    {
        _state.MarkComplete(rootKey);
        _state.ClearBuilding(drive);
        StatusChanged?.Invoke();
        IndexRefreshed?.Invoke(rootKey);
    }

    public void AbandonBuilding(string drive)
    {
        _state.ClearBuilding(drive);
        StatusChanged?.Invoke();
    }

    public void Dispose() { }
}

/// <summary>
/// The app end of the elevated-operation pipe, driven by hand. The sibling of
/// <see cref="ProtocolPeer"/>, and it exists for the same reason: the only thing standing between a
/// test and the real helper is a UAC prompt, which no test can answer.
/// </summary>
internal sealed class ElevationPeer
{
    private readonly Stream _stream;
    private readonly LineReader _reader;

    public ElevationPeer(Stream stream)
    {
        _stream = stream;
        _reader = new LineReader(stream, 64 * 1024);
    }

    public void Send(ElevationVerb verb, string payload = "") =>
        LineChannel.WriteLine(_stream, ElevationProtocol.Format(new ElevationMessage(verb, payload)));

    /// <summary>Sends a line the protocol will refuse, to prove a session survives one.</summary>
    public void SendRaw(string line) => LineChannel.WriteLine(_stream, line);

    /// <summary>Greets, then sends a whole request in the order the client does.</summary>
    public void Request(string header, params string[] items)
    {
        Send(ElevationVerb.Hello, ElevationProtocol.ProtocolVersion.ToString());
        Send(ElevationVerb.Begin, header);
        foreach (var item in items) Send(ElevationVerb.Item, item);
        Send(ElevationVerb.Go);
    }

    /// <summary>The next parseable message, skipping anything malformed. Null at end of stream.</summary>
    public ElevationMessage? Receive()
    {
        while (true)
        {
            string? line;
            try
            {
                line = _reader.ReadLine();
            }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException)
            {
                return null;
            }

            if (line is null) return null;
            if (ElevationProtocol.TryParse(line, out var message)) return message;
        }
    }

    /// <summary>Everything the helper says until it stops, which is what a whole run looks like from
    /// this end.</summary>
    public List<ElevationMessage> ReceiveAll()
    {
        var messages = new List<ElevationMessage>();
        while (Receive() is { } message) messages.Add(message);
        return messages;
    }

    public void Close() => _stream.Dispose();
}
