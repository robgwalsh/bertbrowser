using System.Collections.Concurrent;
using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChecksumRunnerTests
{
    private static ChecksumOutcome Run(
        FakeDigester digester,
        IReadOnlyList<string> paths,
        IProgress<ChecksumProgress>? progress = null,
        CancellationToken ct = default) =>
        new ChecksumRunner(digester).Run(paths, [ChecksumAlgorithm.Sha256], sizes: null, progress, ct);

    [Fact]
    public void EveryFileGetsARow()
    {
        var digester = new FakeDigester { [@"C:\a.bin"] = "AA", [@"C:\b.bin"] = "BB" };

        var outcome = Run(digester, [@"C:\a.bin", @"C:\b.bin"]);

        Assert.Equal(2, outcome.Rows.Count);
        Assert.False(outcome.Cancelled);
        Assert.False(outcome.Incomplete);
    }

    /// <summary>
    /// Four threads finish in whatever order they like; the window must not. A selection shown once
    /// and hashed twice has to come back the same way round both times.
    /// </summary>
    [Fact]
    public void RowsComeBackInInputOrder()
    {
        var paths = Enumerable.Range(0, 200).Select(i => $@"C:\file{i:000}.bin").ToList();
        var digester = new FakeDigester();
        foreach (var path in paths) digester[path] = "AA";

        var outcome = Run(digester, paths);

        Assert.Equal(paths, outcome.Rows.Select(r => r.Path));
    }

    /// <summary>One item's failure never affects the others — the rule every multi-item operation
    /// in this app follows.</summary>
    [Fact]
    public void AnUnreadableFileIsOneFailedRow_AndTheRestSurvive()
    {
        var digester = new FakeDigester { [@"C:\a.bin"] = "AA", [@"C:\c.bin"] = "CC" };

        var outcome = Run(digester, [@"C:\a.bin", @"C:\b.bin", @"C:\c.bin"]);

        Assert.Equal(3, outcome.Rows.Count);
        Assert.Equal(1, outcome.Failures);
        Assert.True(outcome.Incomplete);

        Assert.True(outcome.Rows.Single(r => r.Path == @"C:\b.bin").Failed);
        Assert.Equal("AA", outcome.Rows.Single(r => r.Path == @"C:\a.bin").Digest(ChecksumAlgorithm.Sha256));
    }

    [Fact]
    public void ACancelKeepsWhatWasFinished()
    {
        var paths = Enumerable.Range(0, 500).Select(i => $@"C:\file{i:000}.bin").ToList();
        using var cts = new CancellationTokenSource();

        var digester = new FakeDigester { AfterEachFile = done => { if (done >= 20) cts.Cancel(); } };
        foreach (var path in paths) digester[path] = "AA";

        var outcome = Run(digester, paths, ct: cts.Token);

        Assert.True(outcome.Cancelled);
        Assert.NotEmpty(outcome.Rows);
        Assert.True(outcome.Rows.Count < paths.Count, "a cancel should stop short of the whole list");
    }

    /// <summary>
    /// The coalescing. The digester reports every chunk from four threads; if each of those became
    /// an <see cref="IProgress{T}"/> callback the dispatcher would stop repainting.
    /// </summary>
    [Fact]
    public void ProgressIsCoalesced_FarBelowOneReportPerChunk()
    {
        var paths = Enumerable.Range(0, 50).Select(i => $@"C:\file{i:00}.bin").ToList();
        var digester = new FakeDigester { ChunksPerFile = 40 };
        foreach (var path in paths) digester[path] = "AA";

        var reports = 0;
        Run(digester, paths, new Progress<ChecksumProgress>(_ => Interlocked.Increment(ref reports)));

        // 50 files x 40 chunks = 2,000 opportunities. A 100 ms beat over a run this fast should
        // produce a small handful; anything approaching the chunk count means the throttle is gone.
        Assert.True(reports < 200, $"expected coalesced progress, got {reports} reports");
    }

    [Fact]
    public void TheAlgorithmsAreNormalisedOnce()
    {
        var digester = new FakeDigester { [@"C:\a.bin"] = "AA" };

        var outcome = new ChecksumRunner(digester).Run(
            [@"C:\a.bin"],
            [ChecksumAlgorithm.Sha512, ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha512],
            sizes: null, progress: null, CancellationToken.None);

        Assert.Equal([ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha512], outcome.Algorithms);
    }

    [Fact]
    public void AnEmptySelectionIsAnEmptyOutcome()
    {
        var outcome = Run(new FakeDigester(), []);

        Assert.Empty(outcome.Rows);
        Assert.False(outcome.Cancelled);
        Assert.False(outcome.Incomplete);
    }

    /// <summary>
    /// In-memory <see cref="IFileDigester"/>. A path it does not hold is an unreadable file — null,
    /// not a throw — which is the contract the real one keeps.
    /// </summary>
    private sealed class FakeDigester : IFileDigester
    {
        private readonly ConcurrentDictionary<string, string> _digests = new(StringComparer.OrdinalIgnoreCase);
        private int _done;

        public int ChunksPerFile { get; init; } = 1;
        public Action<int>? AfterEachFile { get; init; }

        public string this[string path] { set => _digests[path] = value; }

        public FileDigests? Digest(
            string path,
            IReadOnlyList<ChecksumAlgorithm> algorithms,
            Action<long>? progress,
            CancellationToken ct)
        {
            ct.ThrowIfCancellationRequested();

            if (!_digests.TryGetValue(path, out var digest)) return null;

            for (var i = 0; i < ChunksPerFile; i++)
            {
                ct.ThrowIfCancellationRequested();
                progress?.Invoke(1024);
            }

            AfterEachFile?.Invoke(Interlocked.Increment(ref _done));

            var byAlgorithm = new Dictionary<ChecksumAlgorithm, string>();
            foreach (var algorithm in algorithms) byAlgorithm[algorithm] = digest;

            return new FileDigests(byAlgorithm, ChunksPerFile * 1024L);
        }
    }
}
