using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The in-memory half of an MFT build over the invented disk: parsing a record's bytes, resolving
/// every directory's path from parent links, and rolling file sizes up the tree.
/// </summary>
/// <remarks>
/// The raw volume read itself needs an administrator token and a real NTFS volume, so it is not here;
/// everything after the bytes arrive is, and that is where the memoization and the allocation
/// patterns live.
/// </remarks>
public class MftBenchmarks
{
    private const int ParseSample = 1_000;

    private List<MftFileRecord> _records = [];
    private Dictionary<ulong, MftNode> _directories = [];
    private Dictionary<ulong, (string Path, bool Hidden)> _resolved = [];
    private byte[][] _recordBytes = [];

    [GlobalSetup]
    public void Setup()
    {
        var model = BenchContext.FromEnvironment().Model;
        (_records, _directories) = SyntheticMft.Build(model);

        _resolved = new Dictionary<ulong, (string, bool)>(_directories.Count);
        foreach (var frn in _directories.Keys)
            MftPathBuilder.TryResolve(_directories, frn, SyntheticPaths.Drive, _resolved, out _, out _, NtfsLayout.RootRecordNumber);

        _recordBytes = _records.Take(ParseSample).Select(r => SyntheticMft.FileRecordBytes(in r)).ToArray();
    }

    /// <summary>Every directory on the volume, into an empty cache: the memoized walk up to the root.</summary>
    [Benchmark]
    public int ResolveAll()
    {
        var cache = new Dictionary<ulong, (string Path, bool Hidden)>(_directories.Count);
        var resolved = 0;
        foreach (var frn in _directories.Keys)
        {
            if (MftPathBuilder.TryResolve(_directories, frn, SyntheticPaths.Drive, cache, out _, out _, NtfsLayout.RootRecordNumber))
                resolved++;
        }

        return resolved;
    }

    /// <summary>Bucket every file onto its parent, then fold the totals up the tree.</summary>
    [Benchmark]
    public List<DirSizeResult> AddAndBuild()
    {
        var builder = new MftDirectorySizeBuilder();
        foreach (var record in _records) builder.Add(in record);
        return builder.Build(SyntheticPaths.Drive, SyntheticPaths.Today, _resolved);
    }

    /// <summary>A thousand FILE records through the parser — fixup, attribute walk, name and size.</summary>
    [Benchmark]
    public int ParseRecords1k()
    {
        var parsed = 0;
        for (var i = 0; i < _recordBytes.Length; i++)
        {
            // The parser applies the update-sequence fixup in place, so it gets a copy each time.
            var copy = (byte[])_recordBytes[i].Clone();
            if (MftReader.TryParseFileRecord(copy, _records[i].RecordNumber, 1024, 512, out _)) parsed++;
        }

        return parsed;
    }
}
