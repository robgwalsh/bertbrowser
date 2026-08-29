using System.Collections.Concurrent;
using System.Diagnostics;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.Duplicates;

/// <summary>
/// Finding files that are byte-for-byte the same, in three passes that each cost far less than the
/// one after it.
/// </summary>
/// <remarks>
/// <para>
/// <b>The shortlist is free.</b> Two files of different lengths cannot be duplicates, and the MFT
/// index already holds a length for every file on every fixed volume — so the pass that would
/// otherwise mean walking millions of directory entries is a query. Only the collisions are ever
/// opened, and only the survivors of a 64 KB sample are ever read in full.
/// </para>
/// <para>
/// <b>Cancelling gives back a floor, not nothing.</b> Whatever grouped before the stop is really
/// grouped — those files were read and their hashes really do match — so it is reported, with
/// <see cref="DuplicateScanOutcome.Cancelled"/> saying the list is short. The one thing that must
/// not happen is emitting a group whose members only agreed on their <em>first 64 KB</em>, so a
/// cancel during sampling reports only the files small enough to have been hashed in full by it.
/// </para>
/// <para>
/// Synchronous, like the planners and executors: <see cref="DuplicateFinder"/> is the async facade
/// that keeps view models off it.
/// </para>
/// </remarks>
public sealed class DuplicateScanner(IDuplicateCandidateSource candidates, IFileHasher hasher)
{
    /// <summary>
    /// How many files are read at once.
    /// </summary>
    /// <remarks>
    /// SHA-256 is hardware-accelerated at a gigabyte or two a second per core, which is slower than
    /// an NVMe disk can feed one thread — so this is worth having. It stays small because the other
    /// case is a spinning disk, where too many concurrent readers turn one sequential pass into
    /// seek thrashing. Candidates arrive in path order (the index is clustered on it) and are
    /// consumed in that order, which keeps each worker's reads local.
    /// </remarks>
    public const int MaxParallelism = 4;

    /// <param name="isBuilding">Whether any volume's initial enumeration is still running.</param>
    /// <param name="isIndexed">Whether this scope's volume reports a complete live index.</param>
    /// <remarks>
    /// The two index facts are passed in rather than resolved here, exactly as
    /// <c>DiskUsageService</c> passes them to <c>DiskUsageRules</c>: it keeps this class free of
    /// <c>IMftIndexService</c> and lets every availability case be tested by handing it a pair of
    /// booleans.
    /// </remarks>
    public DuplicateScanOutcome Scan(
        DuplicateScanRequest request,
        bool isBuilding,
        bool isIndexed,
        CancellationToken ct = default,
        IProgress<DuplicateScanProgress>? progress = null)
    {
        ArgumentNullException.ThrowIfNull(request);

        var run = new Run(progress);

        run.BeginPhase(DuplicateScanPhase.Shortlisting, 0, 0);

        DuplicateShortlist shortlist;
        try
        {
            shortlist = candidates.Shortlist(request, ct);
        }
        catch (OperationCanceledException)
        {
            return new DuplicateScanOutcome([], DuplicateScanAvailability.Ready, Cancelled: true);
        }

        var availability = DuplicateRules.Classify(
            shortlist.FilesInScope, shortlist.SizedFilesInScope, isBuilding, isIndexed);

        // Rows that cannot be believed are not a result set, they are the absence of one. Handing
        // them back would leave the caller re-deciding whether to hash a whole disk for nothing.
        if (availability is DuplicateScanAvailability.NotIndexed or DuplicateScanAvailability.NoSizeData)
            return DuplicateScanOutcome.Empty(availability);

        if (shortlist.Files.Count == 0) return DuplicateScanOutcome.Empty(availability);

        var incomplete = 0;
        var cancelled = false;

        // --- pass two: the first 64 KB of every candidate ---
        var sampleBytes = shortlist.Files.Sum(f => Math.Min(f.SizeBytes, DuplicateRules.HeadSampleBytes));
        run.BeginPhase(DuplicateScanPhase.Sampling, shortlist.Files.Count, sampleBytes);

        var samples = new ConcurrentBag<Sample>();
        cancelled |= !HashAll(shortlist.Files, DuplicateRules.HeadSampleBytes, samples, run, ref incomplete, ct);

        // A file the sample read to its end was hashed in full, so its verdict is already final and
        // survives a cancel. One exactly the sample's length is not: there may be more after it.
        var settled = new List<Sample>();
        var provisional = new List<Sample>();
        foreach (var sample in samples)
            (sample.BytesRead < DuplicateRules.HeadSampleBytes ? settled : provisional).Add(sample);

        var groups = new List<DuplicateGroup>();
        groups.AddRange(Confirm(settled));

        // --- pass three: the survivors, in full ---
        if (!cancelled)
        {
            // Only files whose head still collides with another's are worth reading whole.
            var contenders = provisional
                .GroupBy(s => (s.File.SizeBytes, s.Hash))
                .Where(g => g.Count() > 1)
                .SelectMany(g => g)
                .Select(s => s.File)
                .OrderBy(f => f.DisplayPath, StringComparer.Ordinal)
                .ToList();

            if (contenders.Count > 0)
            {
                run.BeginPhase(DuplicateScanPhase.Hashing, contenders.Count, contenders.Sum(f => f.SizeBytes));

                var full = new ConcurrentBag<Sample>();
                cancelled |= !HashAll(contenders, 0, full, run, ref incomplete, ct);
                groups.AddRange(Confirm(full));
            }
        }

        run.Finished();

        groups.Sort((a, b) => b.WastedBytes.CompareTo(a.WastedBytes));

        return new DuplicateScanOutcome(
            groups,
            availability,
            Cancelled: cancelled,
            Incomplete: incomplete > 0,
            FilesHashed: run.FilesHashed,
            BytesHashed: run.TotalBytesHashed);
    }

    /// <summary>
    /// Hashes every file, bounded-parallel. Returns false when the run was cancelled part-way —
    /// whatever landed in <paramref name="into"/> before that is still good.
    /// </summary>
    private bool HashAll(
        IReadOnlyList<SearchHit> files,
        long maxBytes,
        ConcurrentBag<Sample> into,
        Run run,
        ref int incomplete,
        CancellationToken ct)
    {
        var failures = 0;
        var options = new ParallelOptions
        {
            MaxDegreeOfParallelism = MaxParallelism,
            CancellationToken = ct,
        };

        // Bound once rather than per file: the hasher takes an Action and would otherwise allocate
        // a closure for every candidate, of which there can be hundreds of thousands.
        var addBytes = run.AddBytes;

        try
        {
            Parallel.ForEach(files, options, file =>
            {
                var fingerprint = hasher.Hash(file.DisplayPath, maxBytes, addBytes, ct);
                if (fingerprint is null)
                    Interlocked.Increment(ref failures);
                else
                    into.Add(new Sample(file, fingerprint.Hash, fingerprint.BytesRead, fingerprint.Identity));

                run.FileDone(file.Name);
            });
        }
        catch (OperationCanceledException)
        {
            Interlocked.Add(ref incomplete, failures);
            return false;
        }

        Interlocked.Add(ref incomplete, failures);
        return true;
    }

    /// <summary>
    /// Turns fully-hashed files into groups: same real byte count, same digest, at least two of
    /// them once hardlinks have been folded together.
    /// </summary>
    private static IEnumerable<DuplicateGroup> Confirm(IEnumerable<Sample> samples) =>
        samples
            // BytesRead rather than the indexed size: the row may be stale, and a file that shrank
            // since it was indexed must not be compared against one that did not.
            .GroupBy(s => (s.BytesRead, s.Hash))
            .Where(g => g.Count() > 1)
            .Select(g => Build(g.Key.BytesRead, g.Key.Hash, [.. g]))
            .OfType<DuplicateGroup>();

    /// <summary>
    /// One group, with every set of names belonging to a single file on disk collapsed into one
    /// entry.
    /// </summary>
    /// <returns>
    /// Null when folding leaves fewer than two entries — the "copies" were all one file wearing
    /// several names. Deleting any of them would free nothing, so this is not a result, and it is
    /// what stops a hardlink-heavy tree burying everything else.
    /// </returns>
    private static DuplicateGroup? Build(long sizeBytes, string hash, IReadOnlyList<Sample> members)
    {
        var ordered = members
            .OrderBy(m => m.File.DisplayPath, StringComparer.Ordinal)
            .ToList();

        var entries = new List<DuplicateFile>();
        var claimed = new Dictionary<FileIdentity, int>();

        foreach (var member in ordered)
        {
            if (member.Identity is { } identity)
            {
                if (claimed.TryGetValue(identity, out var at))
                {
                    // Another name for a file already listed. It travels with that entry.
                    entries[at] = entries[at] with
                    {
                        HardlinkPaths = [.. entries[at].HardlinkPaths, member.File.DisplayPath],
                    };
                    continue;
                }
                claimed[identity] = entries.Count;
            }

            entries.Add(new DuplicateFile(
                member.File.DisplayPath,
                member.File.RelativeDirDisplay,
                member.File.Name,
                sizeBytes,
                member.File.ModifiedUtc,
                member.File.Hidden,
                []));
        }

        return entries.Count > 1 ? new DuplicateGroup(sizeBytes, hash, entries) : null;
    }

    private sealed record Sample(SearchHit File, string Hash, long BytesRead, FileIdentity? Identity);

    /// <summary>
    /// One <see cref="Scan"/> call's running counters and the rate at which it is willing to talk
    /// about them.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>The coalescing is not optional</b>, for the reason <c>TransferExecutor.Run</c> documents:
    /// the hasher reports every megabyte, and forwarding each one to an <see cref="IProgress{T}"/>
    /// bound to the UI floods the dispatcher with work whose only job is to move a bar a pixel.
    /// Phase boundaries always report; in between, at most one report per
    /// <see cref="ReportInterval"/>.
    /// </para>
    /// <para>
    /// Unlike the transfer's, this one is written to by several threads at once, so the counters go
    /// through <see cref="Interlocked"/> and the throttle is behind a lock. The lock is entered
    /// roughly once per megabyte read, which is nothing beside reading the megabyte.
    /// </para>
    /// </remarks>
    private sealed class Run(IProgress<DuplicateScanProgress>? progress)
    {
        private static readonly TimeSpan ReportInterval = TimeSpan.FromMilliseconds(100);

        private readonly Stopwatch _sinceReport = Stopwatch.StartNew();
        private readonly object _gate = new();

        private DuplicateScanPhase _phase = DuplicateScanPhase.Shortlisting;
        private int _done;
        private int _total;
        private long _bytesDone;
        private long _bytesTotal;
        private long _totalBytesHashed;
        private int _filesHashed;
        private string _name = "";

        /// <summary>Across every phase, for the outcome — <see cref="_bytesDone"/> resets each time.</summary>
        internal long TotalBytesHashed => Interlocked.Read(ref _totalBytesHashed);

        internal int FilesHashed => _filesHashed;

        /// <summary>
        /// Each phase carries its own totals and starts from zero.
        /// </summary>
        /// <remarks>
        /// Sampling's byte total is known exactly; the full pass's cannot be, until sampling has
        /// said who survived it. Two determinate bars in sequence is the honest shape — a single
        /// one would have to invent a denominator and then revise it downwards.
        /// </remarks>
        internal void BeginPhase(DuplicateScanPhase phase, int total, long bytesTotal)
        {
            lock (_gate)
            {
                _phase = phase;
                _total = total;
                _bytesTotal = bytesTotal;
                _done = 0;
                _bytesDone = 0;
                _name = "";
            }
            Report(force: true);
        }

        /// <summary>Chunk deltas from the hasher, from any of the reading threads.</summary>
        internal void AddBytes(long delta)
        {
            Interlocked.Add(ref _bytesDone, delta);
            Interlocked.Add(ref _totalBytesHashed, delta);
            Report(force: false);
        }

        internal void FileDone(string name)
        {
            Interlocked.Increment(ref _done);
            Interlocked.Increment(ref _filesHashed);
            _name = name;
            Report(force: false);
        }

        internal void Finished()
        {
            _name = "";
            Report(force: true);
        }

        private void Report(bool force)
        {
            if (progress is null) return;

            DuplicateScanProgress snapshot;
            lock (_gate)
            {
                if (!force && _sinceReport.Elapsed < ReportInterval) return;
                _sinceReport.Restart();

                snapshot = new DuplicateScanProgress(
                    _phase,
                    Volatile.Read(ref _done),
                    _total,
                    _name,
                    Interlocked.Read(ref _bytesDone),
                    _bytesTotal);
            }

            progress.Report(snapshot);
        }
    }
}
