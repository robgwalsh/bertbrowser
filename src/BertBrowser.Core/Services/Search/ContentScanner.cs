using System.Collections.Concurrent;
using System.Diagnostics;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Search;

/// <summary>What the reading pass produced.</summary>
/// <param name="Hits">The survivors, capped.</param>
/// <param name="Truncated">The result cap was reached.</param>
/// <param name="Cancelled">The user stopped it; <paramref name="Hits"/> is a floor.</param>
/// <param name="Report">What was actually read, for saying so out loud.</param>
public sealed record ContentScanOutcome(
    IReadOnlyList<SearchHit> Hits,
    bool Truncated,
    bool Cancelled,
    ContentScanReport Report);

/// <summary>
/// The second pass of a <c>content:</c> search: takes the shortlist the name and metadata produced
/// and decides it by reading files.
/// </summary>
/// <remarks>
/// <para>Modelled on <c>SearchService.ScanArchivesAsync</c> and <c>DuplicateScanner.HashAll</c>,
/// which is where this app already keeps "open a lot of files, stay cancellable, report a floor".
/// Synchronous, like the duplicate scanner; <c>SearchService</c> is the async facade.</para>
/// <para><strong>Only undecided candidates are ever opened.</strong> That is the whole point of the
/// verdict being three-valued: in <c>content:a OR ext:md</c> every <c>.md</c> is already a hit and
/// goes straight to the results, and in <c>is:dir content:x</c> every directory is already refused.
/// Spending the file ceiling re-establishing what the name settled would be the expensive way to
/// learn nothing.</para>
/// <para><strong>A cancel gives back a floor, not nothing.</strong> Whatever matched before the
/// stop really did match — those files were read — so it is reported, with
/// <see cref="ContentScanOutcome.Cancelled"/> saying the list is short. A cancelled search that
/// returned an empty list would blank the results the user was watching fill up.</para>
/// </remarks>
public sealed class ContentScanner(IContentReader reader)
{
    /// <param name="candidates">The first pass's hits, name and metadata already applied.</param>
    /// <param name="maxResults">The result cap — separate from the file ceiling.</param>
    /// <param name="candidatesTruncated">Whether the first pass itself filled up.</param>
    /// <param name="maxBytesPerFile">How much of each file to read; the one bound a person can set.</param>
    public ContentScanOutcome Scan(
        SearchQuery query,
        IReadOnlyList<SearchHit> candidates,
        int maxResults,
        bool candidatesTruncated,
        long maxBytesPerFile,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches,
        IProgress<ContentScanProgress>? progress,
        CancellationToken ct)
    {
        ArgumentNullException.ThrowIfNull(query);
        ArgumentNullException.ThrowIfNull(candidates);

        var needles = query.ContentNeedles;

        // One cheap pass with no I/O at all, to separate what the name already settled from what
        // has to be read. Emitting the settled ones first is also why a query like
        // "content:todo OR ext:md" puts rows on screen instantly.
        var settled = new List<SearchHit>();
        var toRead = new List<SearchHit>();
        foreach (var hit in candidates)
        {
            switch (query.Evaluate(Candidate(hit, content: null)))
            {
                case SearchMatch.Yes: settled.Add(hit); break;
                case SearchMatch.NeedsContent: toRead.Add(hit); break;
            }
        }

        var hits = new List<SearchHit>();
        var run = new Run(progress, toRead.Count);

        if (settled.Count > 0)
        {
            var take = settled.Count > maxResults ? settled.GetRange(0, maxResults) : settled;
            hits.AddRange(take);
            liveBatches?.Report([.. take]);
        }

        var limit = candidatesTruncated ? ContentScanLimit.Candidates : ContentScanLimit.None;
        var cancelled = false;
        var unreadable = 0;
        var truncatedFiles = 0;
        var lastExamined = candidates.Count > 0 ? candidates[^1].DisplayPath : null;

        if (hits.Count < maxResults && toRead.Count > 0)
        {
            var found = new ConcurrentBag<SearchHit>();
            var stop = 0;

            var options = new ParallelOptions
            {
                MaxDegreeOfParallelism = ContentSearchRules.MaxParallelism,
                CancellationToken = ct,
            };

            try
            {
                Parallel.ForEach(toRead, options, hit =>
                {
                    if (Volatile.Read(ref stop) != 0) return;

                    // Both ceilings are claimed before the open, not after, so the count cannot
                    // overshoot by the number of threads.
                    if (!run.TryClaimFile())
                    {
                        Interlocked.CompareExchange(ref stop, (int)ContentScanLimit.Files, 0);
                        return;
                    }
                    if (run.BytesRead >= ContentSearchRules.MaxBytesRead)
                    {
                        Interlocked.CompareExchange(ref stop, (int)ContentScanLimit.Bytes, 0);
                        return;
                    }

                    var text = reader.Read(hit.DisplayPath, maxBytesPerFile, ct);
                    if (text is null)
                    {
                        // This file had a problem; the rest are unaffected, which is the contract
                        // every batch operation in this app keeps.
                        Interlocked.Increment(ref unreadable);
                        run.FileDone(hit.DisplayPath, 0);
                        return;
                    }

                    run.FileDone(hit.DisplayPath, text.Text.Length);
                    if (text.Truncated) Interlocked.Increment(ref truncatedFiles);

                    if (query.Evaluate(Candidate(hit, text)) != SearchMatch.Yes) return;

                    found.Add(hit with { Match = ContentSnippet.For(text, needles) });

                    // The text goes out of scope here and must: at the file ceiling, holding one
                    // per hit would be tens of thousands of megabyte strings alive at once.
                });
            }
            catch (OperationCanceledException)
            {
                cancelled = true;
            }

            if (limit == ContentScanLimit.None && stop != 0) limit = (ContentScanLimit)stop;

            // Ordered by path so a run is repeatable: Parallel.ForEach completes in whatever order
            // the disk answers, and a result list that reshuffled between two identical searches
            // would be impossible to trust.
            var ordered = found
                .OrderBy(h => PathKey.Canonicalize(h.DisplayPath), StringComparer.Ordinal)
                .ToList();

            foreach (var hit in ordered)
            {
                if (hits.Count >= maxResults) break;
                hits.Add(hit);
            }

            if (ordered.Count > 0)
                liveBatches?.Report(ordered.Take(Math.Max(0, maxResults - settled.Count)).ToArray());

            lastExamined = run.LastPath ?? lastExamined;
        }

        run.Finished();

        var report = new ContentScanReport(
            run.FilesRead, run.BytesRead, limit, unreadable, truncatedFiles, lastExamined);

        return new ContentScanOutcome(
            hits,
            Truncated: hits.Count >= maxResults,
            Cancelled: cancelled,
            Report: report);
    }

    private static SearchCandidate Candidate(SearchHit hit, ContentText? content) =>
        new(hit.Name.ToUpperInvariant(),
            PathKey.Canonicalize(hit.DisplayPath),
            hit.IsDirectory,
            hit.SizeBytes,
            hit.ModifiedUtc,
            hit.Hidden,
            content);

    /// <summary>
    /// The counters, and the throttle in front of the progress callback.
    /// </summary>
    /// <remarks>
    /// The coalescing is not optional, for the reason <c>DuplicateScanner.Run</c> and
    /// <c>TransferExecutor.Run</c> both give: a report per file at thousands of files a second
    /// floods the dispatcher with work whose only job is to move a number. At most one report per
    /// 100 ms in between, and the finish always reports. Several threads write these, so the
    /// counters go through <see cref="Interlocked"/> and the throttle sits behind a lock.
    /// </remarks>
    private sealed class Run(IProgress<ContentScanProgress>? progress, int total)
    {
        private static readonly TimeSpan ReportInterval = TimeSpan.FromMilliseconds(100);

        private readonly Stopwatch _sinceReport = Stopwatch.StartNew();
        private readonly Lock _gate = new();
        private int _claimed;
        private int _filesRead;
        private long _bytesRead;
        private string? _lastPath;

        public int FilesRead => Volatile.Read(ref _filesRead);
        public long BytesRead => Interlocked.Read(ref _bytesRead);
        public string? LastPath { get { lock (_gate) return _lastPath; } }

        /// <summary>Takes one from the file ceiling, or refuses when it is spent.</summary>
        public bool TryClaimFile() =>
            Interlocked.Increment(ref _claimed) <= ContentSearchRules.MaxFilesOpened;

        public void FileDone(string path, long bytes)
        {
            Interlocked.Increment(ref _filesRead);
            Interlocked.Add(ref _bytesRead, bytes);
            lock (_gate) _lastPath = path;
            Report(force: false);
        }

        public void Finished() => Report(force: true);

        private void Report(bool force)
        {
            if (progress is null) return;

            ContentScanProgress snapshot;
            lock (_gate)
            {
                if (!force && _sinceReport.Elapsed < ReportInterval) return;
                _sinceReport.Restart();
                snapshot = new ContentScanProgress(
                    Volatile.Read(ref _filesRead), total, Interlocked.Read(ref _bytesRead));
            }

            progress.Report(snapshot);
        }
    }
}
