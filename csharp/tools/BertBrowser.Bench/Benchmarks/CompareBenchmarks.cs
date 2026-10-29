using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Diff;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>Ten thousand lines against the same ten thousand with one in a hundred changed.</summary>
public class DiffBenchmarks
{
    private string[] _left = [];
    private string[] _right = [];

    [GlobalSetup]
    public void Setup()
    {
        _left = Enumerable.Range(0, 10_000).Select(i => $"line {i}: the quick brown fox jumps over the lazy dog {i % 97}").ToArray();
        _right = _left.Select((line, i) => i % 100 == 50 ? line + " (changed)" : line).ToArray();
    }

    [Benchmark]
    public TextDiff Compare10k() => TextDiffer.Compare(_left, _right);
}

/// <summary>
/// Two folders of a hundred thousand entries matched side by side, two percent of them differing;
/// and two 64 MB files compared byte for byte.
/// </summary>
[AllocationTolerance(2)]
public class FolderCompareBenchmarks
{
    private static readonly DateTime Stamp = new(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);

    private CompareEntry[] _left = [];
    private CompareEntry[] _right = [];
    private readonly FileContentComparer _content = new();
    private string _big = "";
    private string _bigCopy = "";

    [GlobalSetup]
    public void Setup()
    {
        var left = new List<CompareEntry>(100_000);
        var right = new List<CompareEntry>(100_000);
        for (var i = 0; i < 100_000; i++)
        {
            var key = $@"D{i % 500:000}\F{i:00000}.TXT";
            var entry = new CompareEntry(key, $"F{i:00000}.txt", false, 1000 + i % 777, Stamp.AddSeconds(i));
            left.Add(entry);
            right.Add((i % 100) switch
            {
                0 => entry with { SizeBytes = entry.SizeBytes + 1 },
                1 => entry with { ModifiedUtc = entry.ModifiedUtc.AddHours(1) },
                _ => entry,
            });
            if (i % 200 == 0) right.Add(new CompareEntry(key + ".ORIG", $"F{i:00000}.txt.orig", false, 10, Stamp));
        }
        _left = left.ToArray();
        _right = right.ToArray();

        var tree = BenchContext.FromEnvironment().Tree;
        _big = tree.Big;
        _bigCopy = tree.BigCopy;
        _content.Compare(_big, _bigCopy, null, CancellationToken.None);
    }

    [Benchmark]
    public CompareResult Compare100k() => FolderComparer.Compare(_left, _right, CompareTolerance.Strict);

    [Benchmark]
    public ContentComparison FileContent64Mb() => _content.Compare(_big, _bigCopy, null, CancellationToken.None);
}
