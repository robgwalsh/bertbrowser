using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Services.Archives;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// Building the tree an archive is browsed through from a hundred thousand flat entries — with and
/// without the duplicate keys a sloppy archiver writes.
/// </summary>
public class ArchiveBenchmarks
{
    [Params(false, true)]
    public bool Dupes { get; set; }

    private RawArchiveEntry[] _entries = [];

    [GlobalSetup]
    public void Setup()
    {
        var stamp = new DateTime(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);
        var entries = new List<RawArchiveEntry>(101_000);
        for (var i = 0; i < 100_000; i++)
        {
            var key = $"d{i % 500}/s{i % 37}/file{i}.txt";
            entries.Add(new RawArchiveEntry(key, 1000 + i % 5000, 400 + i % 2000, stamp.AddSeconds(i), false));
            if (Dupes && i % 100 == 0 && i > 0)
                entries.Add(new RawArchiveEntry($"d{(i - 100) % 500}/s{(i - 100) % 37}/file{i - 100}.txt", 1, 1, stamp, false));
        }
        _entries = entries.ToArray();
    }

    [Benchmark]
    public ArchiveIndex Build100k() => ArchiveIndexBuilder.Build(_entries, ArchiveCapabilities.Unknown);
}

/// <summary>Listing a small real 7z through SharpCompress — the container the fixtures ship.</summary>
[AllocationTolerance(2)]
public class ArchiveReadBenchmarks
{
    private string _sevenZip = "";
    private readonly SharpCompressArchiveReader _reader = new();

    [GlobalSetup]
    public void Setup()
    {
        _sevenZip = Path.Combine(Path.GetTempPath(), "bertbrowser-bench", $"plain-{Guid.NewGuid():N}.7z");
        Directory.CreateDirectory(Path.GetDirectoryName(_sevenZip)!);
        ArchiveFixtures.WriteTo(_sevenZip, ArchiveFixtures.PlainSevenZip);
    }

    [Benchmark]
    public ArchiveIndex ReadPlain7z() => _reader.Read(_sevenZip, null);

    [GlobalCleanup]
    public void Cleanup()
    {
        try { File.Delete(_sevenZip); }
        catch (IOException) { }
    }
}
