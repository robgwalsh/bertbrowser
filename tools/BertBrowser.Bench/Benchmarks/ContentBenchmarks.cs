using System.Text;
using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Fakes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The <c>content:</c> pass over two thousand candidates: once from memory, so the scanner and
/// the four-way parallelism are measured alone, and once from the real <c>Text</c> tree with its
/// binaries and its needles — which adds the reads <em>and</em> the decode ladder, so the gap
/// between the two is not disk alone.
/// </summary>
[AllocationTolerance(5)]
public class ContentBenchmarks
{
    private const long MaxBytesPerFile = 1024 * 1024;
    private static readonly DateTime Stamp = new(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);

    private SearchQuery _query = null!;
    private ContentScanner _inMemory = null!;
    private ContentScanner _disk = null!;
    private List<SearchHit> _memoryCandidates = [];
    private List<SearchHit> _diskCandidates = [];

    [GlobalSetup]
    public void Setup()
    {
        _query = SearchGrammar.Parse($"content:{BenchCorpus.Needle}").Query
            ?? throw new InvalidOperationException("the content query did not parse");

        var rng = new Random(99);
        var table = new Dictionary<string, ContentText>(StringComparer.Ordinal);
        var sb = new StringBuilder();
        for (var i = 0; i < 2_000; i++)
        {
            var path = $@"Q:\Bench\Text\t{i:00000}.txt";
            sb.Clear();
            var size = rng.Next(2 * 1024, 64 * 1024);
            while (sb.Length < size) sb.Append("the index reads every name once and patches it from the journal ");
            if (i % 10 == 0) sb.Insert(size / 2, BenchCorpus.Needle);
            table[path] = new ContentText(sb.ToString(), truncated: false);
            _memoryCandidates.Add(new SearchHit(path, "", $"t{i:00000}.txt", false, size, Stamp));
        }
        _inMemory = new ContentScanner(new InMemoryContentReader(table));

        var context = BenchContext.FromEnvironment();
        var folder = context.Real?.TextFolder ?? context.Tree.Text;
        _diskCandidates = new DirectoryInfo(folder).EnumerateFiles("*", SearchOption.AllDirectories)
            .Where(f => f.Length is > 0 and <= MaxBytesPerFile)
            .Take(2_000)
            .Select(f => new SearchHit(f.FullName, Path.GetRelativePath(folder, f.DirectoryName ?? folder), f.Name, false, f.Length, f.LastWriteTimeUtc))
            .ToList();
        _disk = new ContentScanner(new FileSystemContentReader());
        _disk.Scan(_query, _diskCandidates, 1000, false, MaxBytesPerFile, null, null, CancellationToken.None);
    }

    [Benchmark]
    public ContentScanOutcome ScanInMemory2k() =>
        _inMemory.Scan(_query, _memoryCandidates, 1000, false, MaxBytesPerFile, null, null, CancellationToken.None);

    [Benchmark]
    public ContentScanOutcome ScanDisk2k() =>
        _disk.Scan(_query, _diskCandidates, 1000, false, MaxBytesPerFile, null, null, CancellationToken.None);
}
