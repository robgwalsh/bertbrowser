using System.Windows.Threading;
using BertBrowser.App.Interop;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.Services;

/// <summary>
/// Fills the shell-metadata columns of the rows a list is actually showing.
/// </summary>
/// <remarks>
/// <para>
/// One per file list, holding the columns in force, a bounded cache and the concurrency bound. Every
/// read goes through <see cref="ShellProperties.ReadValues"/>, which refuses a cloud placeholder, a
/// reparse point and a directory before opening anything.
/// </para>
/// <para>
/// <b>A cell asks; it never fetches.</b> <see cref="Value"/> answers from the cache and otherwise
/// records a want and returns null — blank, which is what an unread value must look like. The work
/// happens in one coalesced pass afterwards, and that indirection is the whole design rather than
/// tidiness: <c>Icon</c> and <c>Thumbnail</c> can afford to start a read from their own getter
/// because tiles are few and large, while a details list scrolls an order of magnitude faster. A
/// flick through a big folder realizes and discards thousands of containers a second, and starting
/// a file open for each would leave the disk busy for minutes after the user stopped, every one of
/// them for a row that is long gone.
/// </para>
/// <para>
/// So the pass asks <see cref="RealizedRows"/> what is still on screen and hydrates only that.
/// Rows that scrolled past in between are never opened at all.
/// </para>
/// </remarks>
public sealed class ShellMetadataHydrator : IDisposable
{
    /// <summary>Four at once. The peak <c>ContentSearchRules</c> and <c>DuplicateScanner</c> both
    /// measured for this kind of fan-out, and — since there is no cancelling a COM call once it is
    /// in flight — the only thing standing between a wedged property handler and the thread pool.
    /// A task per row would mean hundreds of concurrent <c>SHGetPropertyStoreFromParsingName</c>
    /// calls, which stops the shell answering for every application on the machine.</summary>
    private const int MaxConcurrentReads = 4;

    /// <summary>A ceiling per pass, so even a pathological realized range cannot queue an unbounded
    /// amount of work before the next one gets a chance to narrow it.</summary>
    private const int MaxRowsPerPass = 200;

    private readonly Dispatcher _dispatcher;
    private readonly ShellMetadataCache _cache = new();
    private readonly SemaphoreSlim _reads = new(MaxConcurrentReads);
    private readonly HashSet<FileItemViewModel> _wanted = [];
    private readonly object _gate = new();

    private CancellationTokenSource _cts = new();
    private IReadOnlyList<string> _keys = [];
    private bool _passScheduled;
    private int _running;
    private bool _disposed;

    public ShellMetadataHydrator(Dispatcher dispatcher) => _dispatcher = dispatcher;

    /// <summary>What the list considers on screen. Set by the view, which is the only thing that can
    /// know; without it the pass falls back to everything asked for, which is correct but unbounded
    /// under a fast scroll.</summary>
    public Func<IReadOnlyCollection<FileItemViewModel>>? RealizedRows { get; set; }

    /// <summary>Raised when values have arrived, so the list can lower its busy flag.</summary>
    public event EventHandler? Idle;

    /// <summary>Whether a pass is in flight. <c>UiSession.Settle</c> waits on this through the file
    /// list, so a scripted capture never photographs half-filled columns.</summary>
    public bool IsBusy
    {
        get { lock (_gate) return _passScheduled || _running > 0; }
    }

    /// <summary>The shell columns currently on the list. Changing them abandons anything in flight —
    /// a pass for columns nobody is showing any more is pure waste.</summary>
    public void SetColumns(IReadOnlyList<string> canonicalNames)
    {
        lock (_gate)
        {
            if (_keys.SequenceEqual(canonicalNames, StringComparer.OrdinalIgnoreCase)) return;
            _keys = canonicalNames;
        }
        Reset();
    }

    /// <summary>Abandons everything in flight — a new listing, or a change of columns.</summary>
    public void Reset()
    {
        CancellationTokenSource old;
        lock (_gate)
        {
            old = _cts;
            _cts = new CancellationTokenSource();
            _wanted.Clear();
        }
        old.Cancel();
        old.Dispose();
    }

    /// <summary>
    /// The cached value or nothing, <b>without asking for it to be read</b>.
    /// </summary>
    /// <remarks>
    /// This is what the sort comparer uses, and the distinction from <see cref="Value"/> is
    /// load-bearing: sorting a folder compares every row against several others, so a comparer that
    /// registered a want would ask to open every file in the folder — hundreds of thousands of them
    /// — to answer a single click on a header. An unread value is simply unknown, and
    /// <c>ColumnComparison</c> sorts unknown last.
    /// </remarks>
    public ColumnValue? Peek(FileItemViewModel row, string canonical) =>
        _cache.Peek(row.FullPath, row.ModifiedUtc, canonical, out _);

    /// <summary>
    /// What this row shows in that column: the cached value, or null and a note to go and read it.
    /// </summary>
    public ColumnValue? Value(FileItemViewModel row, string canonical)
    {
        if (_disposed) return null;

        var cached = _cache.Peek(row.FullPath, row.ModifiedUtc, canonical, out var attempted);
        if (cached is not null || attempted) return cached;

        lock (_gate)
        {
            if (_keys.Count == 0) return null;
            _wanted.Add(row);
            if (_passScheduled) return null;
            _passScheduled = true;
        }

        // Background priority, coalesced to one pass — the QueueSelectionSummary idiom. Everything
        // realized during this turn of the layout lands in the same batch.
        _dispatcher.BeginInvoke(DispatcherPriority.Background, RunPass);
        return null;
    }

    private void RunPass()
    {
        List<FileItemViewModel> batch;
        IReadOnlyList<string> keys;
        CancellationToken token;

        lock (_gate)
        {
            _passScheduled = false;
            keys = _keys;
            token = _cts.Token;

            // The narrowing that makes a fast scroll free: whatever asked while the rows were
            // flying past, only what is still realized is worth opening a file for.
            var realized = RealizedRows?.Invoke();
            batch = realized is null
                ? _wanted.ToList()
                : _wanted.Where(realized.Contains).ToList();
            _wanted.Clear();

            if (batch.Count > MaxRowsPerPass) batch = batch.GetRange(0, MaxRowsPerPass);
            if (batch.Count == 0 || keys.Count == 0) return;
            _running++;
        }

        _ = HydrateAsync(batch, keys, token);
    }

    private async Task HydrateAsync(
        List<FileItemViewModel> batch, IReadOnlyList<string> keys, CancellationToken token)
    {
        try
        {
            var tasks = batch.Select(row => ReadOneAsync(row, keys, token));
            await Task.WhenAll(tasks).ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            // A new listing or a column change superseded this pass; nothing to report.
        }
        finally
        {
            bool idle;
            lock (_gate)
            {
                _running--;
                idle = !_passScheduled && _running == 0;
            }
            if (idle) Idle?.Invoke(this, EventArgs.Empty);
        }
    }

    private async Task ReadOneAsync(
        FileItemViewModel row, IReadOnlyList<string> keys, CancellationToken token)
    {
        await _reads.WaitAsync(token).ConfigureAwait(true);
        try
        {
            if (token.IsCancellationRequested) return;

            var path = row.FullPath;
            var attributes = row.Attributes;
            var modified = row.ModifiedUtc;
            var values = await Task.Run(
                () => ShellProperties.ReadValues(path, attributes, keys), token).ConfigureAwait(true);

            if (token.IsCancellationRequested) return;

            // Recorded even when a property came back with nothing, so a file that genuinely has no
            // Dimensions is not re-opened every time its row scrolls back into view.
            _cache.Store(path, modified, keys, values);
            row.NotifyColumnsChanged();
        }
        finally
        {
            _reads.Release();
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _cts.Cancel();
        _cts.Dispose();
        _reads.Dispose();
    }
}

/// <summary>
/// A bounded cache of read properties, keyed by path and modified time.
/// </summary>
/// <remarks>
/// <para>
/// The <c>PreviewImageCache</c> shape, with two deliberate differences. The key carries no size,
/// because that was a <em>decode parameter</em> for an image rather than part of a file's identity;
/// and the capacity is far larger, because a row of properties is a few hundred bytes where a
/// decoded preview is megabytes — fifty entries would not cover two screenfuls, and scrolling down
/// and back would re-read the lot.
/// </para>
/// <para>
/// The modified time is in the key for the reason it is in that one: a value that outlived an edit
/// would be a lie. It also means the live-refresh <c>Updated</c> path invalidates this for free,
/// with nothing wired between them.
/// </para>
/// </remarks>
internal sealed class ShellMetadataCache
{
    private const int Capacity = 2048;

    private sealed record Entry(Dictionary<string, ColumnValue> Values, HashSet<string> Attempted);

    private readonly Dictionary<string, LinkedListNode<(string Key, Entry Entry)>> _index =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly LinkedList<(string Key, Entry Entry)> _order = new();
    private readonly object _gate = new();

    private static string KeyFor(string path, DateTime modifiedUtc) => $"{path}|{modifiedUtc.Ticks}";

    /// <summary>The cached value, and whether this property has been looked for at all. The second
    /// is what stops a file that genuinely has no value for a column being re-opened on every
    /// scroll.</summary>
    public ColumnValue? Peek(string path, DateTime modifiedUtc, string canonical, out bool attempted)
    {
        lock (_gate)
        {
            if (!_index.TryGetValue(KeyFor(path, modifiedUtc), out var node))
            {
                attempted = false;
                return null;
            }

            _order.Remove(node);
            _order.AddFirst(node);
            attempted = node.Value.Entry.Attempted.Contains(canonical);
            return node.Value.Entry.Values.GetValueOrDefault(canonical);
        }
    }

    public void Store(
        string path, DateTime modifiedUtc,
        IReadOnlyList<string> asked, IReadOnlyDictionary<string, ColumnValue> found)
    {
        var key = KeyFor(path, modifiedUtc);
        lock (_gate)
        {
            if (!_index.TryGetValue(key, out var node))
            {
                node = _order.AddFirst((key, new Entry(new(StringComparer.OrdinalIgnoreCase),
                                                       new(StringComparer.OrdinalIgnoreCase))));
                _index[key] = node;

                while (_index.Count > Capacity && _order.Last is { } last)
                {
                    _index.Remove(last.Value.Key);
                    _order.RemoveLast();
                }
            }
            else
            {
                _order.Remove(node);
                _order.AddFirst(node);
            }

            var entry = node.Value.Entry;
            foreach (var canonical in asked) entry.Attempted.Add(canonical);
            foreach (var (canonical, value) in found) entry.Values[canonical] = value;
        }
    }
}
