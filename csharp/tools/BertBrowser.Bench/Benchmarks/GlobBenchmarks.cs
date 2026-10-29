using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>The wildcard matcher behind every bare search word, over ten thousand names.</summary>
public class GlobBenchmarks
{
    private const string Pattern = "*REP*ORT?.TXT";

    private string[] _names = [];

    [GlobalSetup]
    public void Setup() =>
        _names = BenchContext.FromEnvironment().Model.Files.Take(10_000).Select(f => f.Name.ToUpperInvariant()).ToArray();

    [Benchmark]
    public int WildcardMatch10k()
    {
        var hits = 0;
        foreach (var name in _names)
        {
            if (GlobText.WildcardMatch(name, Pattern)) hits++;
        }

        return hits;
    }
}
