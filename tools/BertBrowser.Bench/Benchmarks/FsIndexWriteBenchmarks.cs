using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The index's write path: one 20,000-row chunk, which is exactly what the MFT pass and the crawler
/// hand to <see cref="FsIndexRepository.UpsertEntries"/> at a time. A two-million-entry volume is a
/// hundred of these.
/// </summary>
/// <remarks>
/// Writes go to a scratch database under <c>%TEMP%</c>, never the shared corpus, and the table is
/// emptied before every iteration so each measurement is a pure insert rather than an update of what
/// the previous one wrote.
/// </remarks>
[AllocationTolerance(2)]
public class FsIndexWriteBenchmarks
{
    private const int ChunkSize = 20_000;

    private string _dbPath = "";
    private Db _db = null!;
    private FsIndexRepository _repo = null!;
    private FsEntryRow[] _chunk = [];

    [GlobalSetup]
    public void Setup()
    {
        _dbPath = BenchContext.ScratchDbPath();
        _db = new Db(_dbPath);
        _db.Migrate();
        _repo = new FsIndexRepository(_db);
        _chunk = SyntheticPaths.Generate(1).Files.Take(ChunkSize).ToArray();
    }

    [IterationSetup]
    public void Empty()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM fs_entry;";
        cmd.ExecuteNonQuery();
    }

    [Benchmark]
    public void Upsert20k() => _repo.UpsertEntries(_chunk, crawlGen: 2);

    [GlobalCleanup]
    public void Cleanup()
    {
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}

/// <summary>
/// The index's two in-place rewrites: the sweep that removes what a completed build did not touch,
/// and the prefix rewrite a directory rename becomes.
/// </summary>
/// <remarks>
/// Both mutate the table, so every iteration starts from the same 100,000 rows — half stamped with
/// the old generation, so the sweep has 50,000 to remove — rebuilt in <c>[IterationSetup]</c>. That
/// costs about a second per iteration and is the price of measuring a destructive operation honestly.
/// </remarks>
[AllocationTolerance(2)]
public class FsIndexMaintenanceBenchmarks
{
    private const long OldGeneration = 1;
    private const long NewGeneration = 2;

    private string _dbPath = "";
    private Db _db = null!;
    private FsIndexRepository _repo = null!;
    private FsEntryRow[] _stale = [];
    private FsEntryRow[] _fresh = [];
    private string _rootKey = "";
    private string _renameFrom = "";
    private string _renameTo = "";

    [GlobalSetup]
    public void Setup()
    {
        _dbPath = BenchContext.ScratchDbPath();
        _db = new Db(_dbPath);
        _db.Migrate();
        _repo = new FsIndexRepository(_db);

        var model = SyntheticPaths.Generate(1);
        var half = model.Rows.Count / 2;
        _stale = model.Rows.Take(half).ToArray();
        _fresh = model.Rows.Skip(half).ToArray();
        _rootKey = PathKey.Canonicalize(SyntheticPaths.Drive);

        // A folder with a few thousand descendants: large enough to be a real rewrite, small enough
        // that the setup is not dominated by it.
        var folder = model.DirectorySizes
            .Where(d => d.FileCount + d.DirCount is >= 2_000 and <= 10_000)
            .OrderByDescending(d => d.FileCount + d.DirCount)
            .First();
        _renameFrom = folder.PathKey;
        _renameTo = folder.PathKey + "-RENAMED";
    }

    [IterationSetup]
    public void Reset()
    {
        using (var conn = _db.Open())
        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText = "DELETE FROM fs_entry;";
            cmd.ExecuteNonQuery();
        }

        foreach (var chunk in _stale.Chunk(20_000)) _repo.UpsertEntries(chunk, OldGeneration);
        foreach (var chunk in _fresh.Chunk(20_000)) _repo.UpsertEntries(chunk, NewGeneration);
    }

    [Benchmark]
    public void SweepVanished50k() => _repo.SweepVanished(_rootKey, NewGeneration);

    [Benchmark]
    public void RenameSubtree() => _repo.Rename(_renameFrom, _renameTo, Path.GetFileName(_renameTo), NewGeneration + 1);

    [GlobalCleanup]
    public void Cleanup()
    {
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}
