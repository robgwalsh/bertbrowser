using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Core.Paths;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// <see cref="PathKey"/> is on every hot path in the app — every DB key, every planner's containment
/// check, every listing diff — so a regression here is a regression everywhere.
/// </summary>
public class PathKeyBenchmarks
{
    private const string Short = @"C:\Users\bench\notes.txt";
    private const string Deep = @"C:\Source\bertbrowser\src\BertBrowser.Core\Services\Transfer\TransferMergeExpander.cs";
    private const string Trailing = @"C:\Source\bertbrowser\src\BertBrowser.Core\Services\Transfer\";
    private const string DeepDir = @"C:\SOURCE\BERTBROWSER\SRC\BERTBROWSER.CORE\SERVICES";

    private string[] _keys = [];
    private string[] _dirs = [];

    [GlobalSetup]
    public void Setup()
    {
        // 1,000 file keys and 50 directory keys from the corpus: the shape of a transfer planner
        // asking "is any of these inside any of those", which is O(keys × dirs) calls.
        var model = SyntheticPaths.Generate(1);
        _keys = model.Files.Take(1_000).Select(f => f.PathKey).ToArray();
        _dirs = model.Directories.Where(d => d.PathKey.Count(c => c == '\\') >= 3).Take(50).Select(d => d.PathKey).ToArray();
    }

    [Benchmark]
    public string CanonicalizeShort() => PathKey.Canonicalize(Short);

    [Benchmark]
    public string CanonicalizeDeep() => PathKey.Canonicalize(Deep);

    [Benchmark]
    public string CanonicalizeTrailing() => PathKey.Canonicalize(Trailing);

    [Benchmark]
    public (string, string) PrefixBounds() => PathKey.PrefixBounds(DeepDir);

    /// <summary>1,000 × 50 containment checks, each of which re-canonicalizes the directory.</summary>
    [Benchmark]
    public int IsUnderLoop()
    {
        var hits = 0;
        foreach (var key in _keys)
        {
            foreach (var dir in _dirs)
            {
                if (PathKey.IsUnder(key, dir)) hits++;
            }
        }

        return hits;
    }
}
