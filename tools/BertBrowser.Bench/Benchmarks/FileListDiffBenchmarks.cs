using BenchmarkDotNet.Attributes;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The refresh diff: what the watcher's 250 ms tick costs over a 10,000-row folder in which one
/// percent changed. Pure — no disk — so this is the two canonicalizations per entry and the
/// dictionary work, and nothing else.
/// </summary>
public class FileListDiffBenchmarks
{
    private const int Rows = 10_000;
    private const int Churn = Rows / 100;

    private FileEntry[] _current = [];
    private FileEntry[] _next = [];

    [GlobalSetup]
    public void Setup()
    {
        var stamp = new DateTime(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);
        FileEntry Entry(int i, long size) => new(
            $"f{i:00000}.txt", $@"C:\Bench\Flat\f{i:00000}.txt", false, size, stamp, FileAttributes.Archive, stamp);

        _current = Enumerable.Range(0, Rows).Select(i => Entry(i, 64)).ToArray();

        // Drop the first hundred, change the size of the next hundred, add a hundred new ones.
        _next = _current.Skip(Churn)
            .Select((e, i) => i < Churn ? e with { SizeBytes = 128 } : e)
            .Concat(Enumerable.Range(Rows, Churn).Select(i => Entry(i, 64)))
            .ToArray();
    }

    [Benchmark]
    public FileListChanges Compute10k() => FileListDiff.Compute(_current, _next);
}
