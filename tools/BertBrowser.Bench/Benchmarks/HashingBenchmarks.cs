using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Fakes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Checksums;
using BertBrowser.Core.Services.Duplicates;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>The five digests over 64 MB already in memory: the algorithms alone, no disk.</summary>
public class DigestBenchmarks
{
    private const int Megabyte = 1024 * 1024;

    [Params(ChecksumAlgorithm.Crc32, ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha1, ChecksumAlgorithm.Sha256, ChecksumAlgorithm.Sha512)]
    public ChecksumAlgorithm Algorithm { get; set; }

    private byte[] _buffer = [];

    [GlobalSetup]
    public void Setup()
    {
        _buffer = new byte[64 * Megabyte];
        new Random(42).NextBytes(_buffer);
    }

    [Benchmark]
    public string Append64Mb()
    {
        using var sink = DigestSinks.Create(Algorithm);
        for (var offset = 0; offset < _buffer.Length; offset += Megabyte)
            sink.Append(_buffer.AsSpan(offset, Megabyte));
        return sink.Finish();
    }
}

/// <summary>
/// The hasher over real files: one 64 MB file end to end, and the 64 KB prefix pass over a hundred
/// — the two reads the duplicate finder makes. The OS has these cached after the first pass, so this
/// is the read loop and the buffer strategy, not the disk.
/// </summary>
[AllocationTolerance(2)]
public class HashBenchmarks
{
    private const long PrefixBytes = 64 * 1024;

    private readonly FileSystemFileHasher _hasher = new();
    private string _big = "";
    private string[] _prefixFiles = [];

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _big = context.Tree.Big;
        _prefixFiles = Directory.GetFiles(context.Tree.Hash, "nearmiss*.bin").Order(StringComparer.Ordinal).ToArray();
        _hasher.Hash(_big, long.MaxValue, null, CancellationToken.None);
    }

    [Benchmark]
    public FileFingerprint? HashFile64Mb() => _hasher.Hash(_big, long.MaxValue, null, CancellationToken.None);

    [Benchmark]
    public int HashPrefix64k()
    {
        var hashed = 0;
        foreach (var file in _prefixFiles)
        {
            if (_hasher.Hash(file, PrefixBytes, null, CancellationToken.None) is not null) hashed++;
        }

        return hashed;
    }
}

/// <summary>
/// The duplicate scanner: its grouping and parallel orchestration over ten thousand in-memory
/// candidates, and the whole thing over the real <c>Hash</c> folder — unique files, identical pairs
/// and the near misses that share a prefix.
/// </summary>
[AllocationTolerance(5)]
public class DuplicateBenchmarks
{
    private static readonly DateTime Stamp = new(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);

    private DuplicateScanner _inMemory = null!;
    private DuplicateScanner _disk = null!;
    private readonly DuplicateScanRequest _request = new(RootPath: null, MinSizeBytes: 1);

    [GlobalSetup]
    public void Setup()
    {
        // 1,000 sizes × 10 files: every file collides by size, and within each size five share one
        // content and five another, so every group of ten becomes two groups of five.
        var hits = new List<SearchHit>(10_000);
        var table = new Dictionary<string, (int ContentId, long Size)>(StringComparer.Ordinal);
        for (var i = 0; i < 10_000; i++)
        {
            var size = 4096L + (i % 1000) * 7;
            var path = $@"Q:\Bench\Dupes\d{i % 100:00}\file{i:00000}.bin";
            hits.Add(new SearchHit(path, $@"d{i % 100:00}", $"file{i:00000}.bin", false, size, Stamp));
            table[path] = ((i % 1000) * 2 + (i / 1000) % 2, size);
        }
        _inMemory = new DuplicateScanner(
            new ListCandidateSource(new DuplicateShortlist(hits, hits.Count, hits.Count)),
            new TableHasher(table));

        var folder = BenchContext.FromEnvironment().Real?.MediaFolder ?? BenchContext.FromEnvironment().Tree.Hash;
        var files = new DirectoryInfo(folder).EnumerateFiles("*", SearchOption.AllDirectories)
            .Where(f => f.Length >= 1)
            .Take(2_000)
            .Select(f => new SearchHit(f.FullName, Path.GetRelativePath(folder, f.DirectoryName ?? folder), f.Name, false, f.Length, f.LastWriteTimeUtc))
            .ToList();
        _disk = new DuplicateScanner(
            new ListCandidateSource(new DuplicateShortlist(files, files.Count, files.Count)),
            new FileSystemFileHasher());
    }

    [Benchmark]
    public DuplicateScanOutcome ScanInMemory10k() => _inMemory.Scan(_request, isBuilding: false, isIndexed: true);

    [Benchmark]
    public DuplicateScanOutcome ScanDisk() => _disk.Scan(_request, isBuilding: false, isIndexed: true);
}
