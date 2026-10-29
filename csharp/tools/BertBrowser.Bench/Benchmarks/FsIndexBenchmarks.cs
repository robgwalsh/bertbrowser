using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Search;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The search queries <c>docs/search-indexing.md</c> measured by hand, run against the index —
/// scoped to one root and across the whole table.
/// </summary>
/// <remarks>
/// Every one of these is a scan with a predicate, never a seek (there is no secondary index, by
/// design), so what a change to the row reader, the GLOB escaping or the ancestor lookup costs shows
/// up here directly. The regex and the empty-result queries are the slow ones on purpose: neither
/// can stop early. Under <c>--real</c> the root is the configured folder and the fixed "today" becomes
/// <c>dm:today</c>.
/// </remarks>
[AllocationTolerance(2)]
public class FsIndexBenchmarks
{
    private const int Cap = 1000;

    [Params(
        "report",
        "ext:txt",
        SyntheticPaths.TodayQuery,
        "size:>100mb",
        "size:>100gb",
        "report ext:cs size:>1kb",
        "re:^img_")]
    public string Query { get; set; } = "";

    private FsIndexRepository _repo = null!;
    private SearchQuery _query = null!;
    private string _root = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _repo = new FsIndexRepository(new Db(context.IndexDbPath, create: false));
        _root = context.SearchRoot;

        var text = context.IsReal && Query == SyntheticPaths.TodayQuery ? "dm:today" : Query;
        _query = SearchGrammar.Parse(text).Query ?? throw new InvalidOperationException($"'{text}' did not parse.");

        // One query first, so the page cache and the connection pool are warm for every benchmark alike.
        _repo.Search(_root, _query, Cap);
    }

    [Benchmark]
    public (IReadOnlyList<SearchHit> Hits, bool Truncated) Search() => _repo.Search(_root, _query, Cap);

    [Benchmark]
    public (IReadOnlyList<SearchHit> Hits, bool Truncated) SearchGlobal() => _repo.SearchGlobal(_query, Cap);

    [GlobalCleanup]
    public void Cleanup() => SqliteConnection.ClearAllPools();
}

/// <summary>The other readers of the same table, none of which take a query.</summary>
[AllocationTolerance(2)]
public class FsIndexScanBenchmarks
{
    private FsIndexRepository _repo = null!;
    private string _root = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _repo = new FsIndexRepository(new Db(context.IndexDbPath, create: false));
        _root = context.SearchRoot;
        _repo.FindCoveringRoot(_root);
    }

    /// <summary>What a folder comparison reads for one side: every row under the root.</summary>
    [Benchmark]
    public (IReadOnlyList<FsSubtreeRow> Rows, bool Truncated) Subtree() => _repo.Subtree(_root, includeHidden: false, cap: 500_000);

    /// <summary>The duplicate finder's two streaming scans, no GROUP BY by design.</summary>
    [Benchmark]
    public DuplicateShortlist DuplicateCandidates() => _repo.DuplicateCandidates(_root, DuplicateScanRequest.DefaultMinSizeBytes, includeHidden: false);

    /// <summary>Asked on every navigation and every search.</summary>
    [Benchmark]
    public FsIndexRoot? FindCoveringRoot() => _repo.FindCoveringRoot(_root);

    [Benchmark]
    public bool HasSizeData() => _repo.HasSizeData(_root);

    [GlobalCleanup]
    public void Cleanup() => SqliteConnection.ClearAllPools();
}
