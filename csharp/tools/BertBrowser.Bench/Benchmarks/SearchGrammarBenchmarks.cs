using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The query language's three costs: parsing on every keystroke, compiling to SQL once per search,
/// and <see cref="SearchQuery.Matches"/> on every row the SQL hands back.
/// </summary>
public class SearchGrammarBenchmarks
{
    private static readonly Lazy<SearchCandidate[]> Candidates = new(() =>
        SyntheticPaths.Generate(1).Files.Take(10_000)
            .Select(f => new SearchCandidate(
                f.Name.ToUpperInvariant(), f.PathKey, f.IsDirectory, f.SizeBytes, f.ModifiedUtc,
                f.Hidden, f.Attributes, f.CreatedUtc))
            .ToArray());

    [Params(
        "report",
        "ext:txt",
        SyntheticPaths.TodayQuery,
        "size:>100mb",
        "size:>100gb",
        "report ext:cs size:>1kb",
        "re:^img_",
        "content:todo OR ext:md",
        "report OR draft OR final ext:docx;pdf;txt size:>1kb !archive dm:2024 path:source is:file")]
    public string Query { get; set; } = "";

    private SearchQuery _parsed = null!;
    private SearchCandidate[] _candidates = [];

    [GlobalSetup]
    public void Setup()
    {
        _parsed = SearchGrammar.Parse(Query).Query ?? throw new InvalidOperationException($"'{Query}' did not parse.");
        _candidates = Candidates.Value;
    }

    [Benchmark]
    public SearchQueryParse Parse() => SearchGrammar.Parse(Query);

    [Benchmark]
    public SqlPredicate Compile() => _parsed.Compile();

    /// <summary>The re-check the repository applies to every row it reads back.</summary>
    [Benchmark]
    public int Matches10k()
    {
        var hits = 0;
        var query = _parsed;
        foreach (ref readonly var candidate in _candidates.AsSpan())
        {
            if (query.Matches(in candidate)) hits++;
        }

        return hits;
    }
}
