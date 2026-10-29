using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The folder-size lookup every listing makes for its directory rows: chunked <c>IN (…)</c> queries
/// of 500, with every key canonicalized on the way in.
/// </summary>
[AllocationTolerance(2)]
public class DirSizeBenchmarks
{
    [Params(50, 2000)]
    public int N { get; set; }

    private DirSizeRepository _repo = null!;
    private string[] _keys = [];

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        var db = new Db(context.IndexDbPath, create: false);
        _repo = new DirSizeRepository(db);

        // Keys straight from the table, so a real index answers with real folders.
        using var conn = db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT path_key FROM dir_size_cache ORDER BY path_key LIMIT @n;";
        cmd.Parameters.AddWithValue("@n", N);
        using var reader = cmd.ExecuteReader();
        var keys = new List<string>(N);
        while (reader.Read()) keys.Add(reader.GetString(0));
        _keys = keys.ToArray();
    }

    [Benchmark]
    public IReadOnlyDictionary<string, DirSizeResult> GetMany() => _repo.GetMany(_keys);

    [GlobalCleanup]
    public void Cleanup() => SqliteConnection.ClearAllPools();
}

/// <summary>The MFT pass's other write: a chunk of folder totals.</summary>
[AllocationTolerance(2)]
public class DirSizeWriteBenchmarks
{
    private string _dbPath = "";
    private Db _db = null!;
    private DirSizeRepository _repo = null!;
    private DirSizeResult[] _chunk = [];

    [GlobalSetup]
    public void Setup()
    {
        _dbPath = BenchContext.ScratchDbPath();
        _db = new Db(_dbPath);
        _db.Migrate();
        _repo = new DirSizeRepository(_db);
        _chunk = BenchContext.FromEnvironment().Model.DirectorySizes.Take(10_000).ToArray();
    }

    [IterationSetup]
    public void Empty()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM dir_size_cache;";
        cmd.ExecuteNonQuery();
    }

    [Benchmark]
    public void UpsertMany10k() => _repo.UpsertMany(_chunk);

    [GlobalCleanup]
    public void Cleanup()
    {
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}
