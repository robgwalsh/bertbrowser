namespace BertBrowser.Core.Services.Checksums;

/// <summary>One file's place in a run.</summary>
/// <param name="Digests">Null when the file could not be read — which is that file's problem alone.</param>
public sealed record ChecksumRow(string Path, string Name, FileDigests? Digests)
{
    public bool Failed => Digests is null;

    public string? Digest(ChecksumAlgorithm algorithm) =>
        Digests is { } d && d.ByAlgorithm.TryGetValue(algorithm, out var digest) ? digest : null;
}

/// <param name="Cancelled">The run stopped early. The rows it did finish are still here.</param>
/// <param name="Failures">Files that could not be read.</param>
public sealed record ChecksumOutcome(
    IReadOnlyList<ChecksumRow> Rows,
    IReadOnlyList<ChecksumAlgorithm> Algorithms,
    bool Cancelled,
    int Failures)
{
    public bool Incomplete => Failures > 0;
}

/// <param name="FilesDone">Files finished, of <paramref name="FilesTotal"/>.</param>
/// <param name="BytesDone">Bytes read, of <paramref name="BytesTotal"/>.</param>
/// <param name="CurrentName">The most recently finished file, for the detail line.</param>
public readonly record struct ChecksumProgress(
    int FilesDone, int FilesTotal, long BytesDone, long BytesTotal, string? CurrentName);

/// <summary>
/// Digesting a selection of files.
/// </summary>
/// <remarks>
/// <para>
/// Shaped after <see cref="Duplicates.DuplicateScanner"/>, deliberately and in the same three
/// places: four threads, a progress callback bound once outside the loop, and a 100 ms coalescing
/// throttle. It is the same problem — many files, one hasher, a UI watching — so a second answer to
/// it would be a second thing to get wrong.
/// </para>
/// <para>
/// One difference: results are placed by input index rather than collected into a bag, so the
/// window shows the files in the order the user selected them and two runs of the same selection
/// cannot come back in different orders.
/// </para>
/// </remarks>
public sealed class ChecksumRunner(IFileDigester digester)
{
    /// <summary>
    /// Four, for <see cref="Duplicates.DuplicateScanner.MaxParallelism"/>'s reasons: hashing is
    /// slower than an NVMe can feed one thread, so more than one helps, and a spinning disk turns
    /// many readers into seek thrash, so not many more.
    /// </summary>
    public const int MaxParallelism = 4;

    public ChecksumOutcome Run(
        IReadOnlyList<string> paths,
        IReadOnlyList<ChecksumAlgorithm> algorithms,
        IReadOnlyDictionary<string, long>? sizes,
        IProgress<ChecksumProgress>? progress,
        CancellationToken ct)
    {
        var wanted = ChecksumAlgorithms.Normalise(algorithms);
        var rows = new ChecksumRow?[paths.Count];

        var bytesTotal = 0L;
        if (sizes is not null)
        {
            foreach (var path in paths)
            {
                if (sizes.TryGetValue(path, out var size)) bytesTotal += size;
            }
        }

        var run = new Beat(progress, paths.Count, bytesTotal);
        var failures = 0;
        var cancelled = false;

        // Bound once: the digester takes an Action and calling it per file would allocate a closure
        // for every one of them.
        var addBytes = run.AddBytes;

        try
        {
            Parallel.For(0, paths.Count,
                new ParallelOptions { MaxDegreeOfParallelism = MaxParallelism, CancellationToken = ct },
                i =>
                {
                    var path = paths[i];
                    var digests = digester.Digest(path, wanted, addBytes, ct);

                    if (digests is null) Interlocked.Increment(ref failures);

                    var name = System.IO.Path.GetFileName(path) is { Length: > 0 } n ? n : path;
                    rows[i] = new ChecksumRow(path, name, digests);
                    run.FileDone(name);
                });
        }
        catch (OperationCanceledException)
        {
            // Whatever finished is still worth showing. A cancel is the user changing their mind,
            // not an error, and throwing away four minutes of completed digests to say so would be
            // the wrong answer to it.
            cancelled = true;
        }

        run.Finished();

        var done = new List<ChecksumRow>(paths.Count);
        foreach (var row in rows)
        {
            if (row is not null) done.Add(row);
        }

        return new ChecksumOutcome(done, wanted, cancelled, failures);
    }

    /// <summary>
    /// Coalesces progress onto a 100 ms beat.
    /// </summary>
    /// <remarks>
    /// Not optional. The digester reports every megabyte, from four threads at once; forwarding each
    /// of those to a UI-bound <see cref="IProgress{T}"/> posts thousands of callbacks a second to the
    /// dispatcher and the window stops repainting. Counters are <see cref="Interlocked"/> because all
    /// four threads write them; the throttle is behind a lock because only one of them should report.
    /// </remarks>
    private sealed class Beat(IProgress<ChecksumProgress>? progress, int filesTotal, long bytesTotal)
    {
        private const int ReportIntervalMs = 100;

        private readonly object _gate = new();
        private long _bytesDone;
        private int _filesDone;
        private string? _current;
        private long _lastReportTicks;

        public void AddBytes(long delta)
        {
            Interlocked.Add(ref _bytesDone, delta);
            Report(force: false);
        }

        public void FileDone(string name)
        {
            Interlocked.Increment(ref _filesDone);
            Volatile.Write(ref _current, name);
            Report(force: false);
        }

        public void Finished() => Report(force: true);

        private void Report(bool force)
        {
            if (progress is null) return;

            lock (_gate)
            {
                var now = Environment.TickCount64;
                if (!force && now - _lastReportTicks < ReportIntervalMs) return;

                _lastReportTicks = now;
                progress.Report(new ChecksumProgress(
                    Volatile.Read(ref _filesDone), filesTotal,
                    Interlocked.Read(ref _bytesDone), bytesTotal,
                    Volatile.Read(ref _current)));
            }
        }
    }
}
