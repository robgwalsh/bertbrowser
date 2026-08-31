namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// One long operation's running counters, its cancellation token, and the rate at which it is
/// willing to talk about them.
/// </summary>
/// <remarks>
/// <para>
/// <b>The coalescing is not optional.</b> <c>CopyFileEx</c> calls back per chunk — thousands of
/// times for one large file — and forwarding each one to an <see cref="IProgress{T}"/> bound to
/// the UI floods the dispatcher with work whose only job is to redraw a bar that has moved a
/// pixel. Item boundaries always report; in between, at most one report per
/// <see cref="ReportInterval"/>. It is the same guard <c>SearchService</c> puts on live results.
/// </para>
/// <para>
/// <b>Bytes are counted per file and snapped at the end of one.</b> A copy may report coarsely
/// or, for a small file, not at all, so the running total is set from the size we already knew
/// rather than from whatever the last callback happened to say. That is what keeps the figure
/// monotonic and makes it add up to the real total at the end.
/// </para>
/// <para>
/// It lives beside <see cref="TransferExecutor"/> rather than inside it because extraction wants
/// exactly this and differs only in where the bytes come from. Two copies of a throttle drift:
/// one would end up reporting at 100 ms and the other at every chunk, and the second would be
/// found by a user rather than by a test.
/// </para>
/// </remarks>
public sealed class ProgressCoalescer
{
    private static readonly TimeSpan ReportInterval = TimeSpan.FromMilliseconds(100);

    private readonly IProgress<TransferProgress>? _progress;
    private readonly int _items;
    private readonly System.Diagnostics.Stopwatch _sinceReport = System.Diagnostics.Stopwatch.StartNew();

    private long _bytesDone;
    private long _fileBase;
    private long _fileBytes;
    private long _fileTotal;
    private int _done;
    private string _name = "";

    public ProgressCoalescer(CancellationToken token, IProgress<TransferProgress>? progress, int items)
    {
        Token = token;
        _progress = progress;
        _items = items;
    }

    /// <summary>A run for work that must finish once started: clearing a name into staging,
    /// putting a displaced entry back, undoing. Reports nothing and cannot be cancelled.</summary>
    public static ProgressCoalescer Silent() => new(CancellationToken.None, null, 0);

    public CancellationToken Token { get; }

    /// <summary>Bytes confirmed transferred so far, across every item.</summary>
    public long BytesDone => _bytesDone;

    public void BeginItem(string name)
    {
        _name = name;
        Report(force: true);
    }

    public void EndItem() => _done++;

    public void Finished()
    {
        _name = "";
        _fileBytes = 0;
        _fileTotal = 0;
        Report(force: true);
    }

    public void BeginFile(long knownLength)
    {
        _fileBase = _bytesDone;
        _fileBytes = 0;
        _fileTotal = knownLength;
    }

    public void FileProgress(long transferred, long total)
    {
        _fileBytes = transferred;
        if (total > _fileTotal) _fileTotal = total;
        _bytesDone = _fileBase + transferred;
        Report(force: false);
    }

    public void EndFile()
    {
        _bytesDone = _fileBase + Math.Max(_fileTotal, _fileBytes);
        _fileBytes = 0;
        _fileTotal = 0;
    }

    private void Report(bool force)
    {
        if (_progress is null) return;
        if (!force && _sinceReport.Elapsed < ReportInterval) return;

        _sinceReport.Restart();
        _progress.Report(new TransferProgress(
            _done, _items, _name, _bytesDone, _fileBytes, _fileTotal));
    }
}
