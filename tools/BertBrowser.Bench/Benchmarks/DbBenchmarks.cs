using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// What one <see cref="Db.Open"/> costs. Every repository method opens its own connection and every
/// open runs the three-PRAGMA batch, so this is a fixed tax on every query in the app.
/// </summary>
[AllocationTolerance(2)]
public class DbBenchmarks
{
    private Db _db = null!;
    private string _rawConnectionString = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _db = new Db(context.IndexDbPath, create: false);
        _rawConnectionString = new SqliteConnectionStringBuilder
        {
            DataSource = context.IndexDbPath,
            Mode = SqliteOpenMode.ReadWrite,
            Pooling = true,
        }.ToString();

        // Prime the pool so neither benchmark pays the first real open.
        using var warm = _db.Open();
    }

    /// <summary>The app's path: pooled connection plus the PRAGMA batch.</summary>
    [Benchmark]
    public void Open()
    {
        using var conn = _db.Open();
    }

    /// <summary>The same pooled connection with no PRAGMAs — the floor the batch sits on.</summary>
    [Benchmark]
    public void OpenWithoutPragmas()
    {
        using var conn = new SqliteConnection(_rawConnectionString);
        conn.Open();
    }

    [GlobalCleanup]
    public void Cleanup() => SqliteConnection.ClearAllPools();
}
