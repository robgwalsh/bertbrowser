using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services.Changes;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// The change timeline's table over 200,000 rows: the recorder's batch write, the window's query, and
/// the prune that runs every minute while recording.
/// </summary>
/// <remarks>
/// <c>Record</c> folds a repeat of the same path and kind within a minute into the existing row, so
/// the batch here is 3,500 new events plus 1,500 repeats — the shape a save-heavy editor produces.
/// The rows it adds are removed before the next iteration, so the table stays at its seeded size.
/// </remarks>
[AllocationTolerance(2)]
public class ChangeLogBenchmarks
{
    private const int SeedRows = 200_000;
    private const int NewEvents = 3_500;
    private const int RepeatEvents = 1_500;

    private static readonly DateTime Now = new(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);
    private static readonly IReadOnlySet<ChangeKind> AllKinds =
        new HashSet<ChangeKind> { ChangeKind.Created, ChangeKind.Modified, ChangeKind.Deleted, ChangeKind.Renamed };

    private string _dbPath = "";
    private Db _db = null!;
    private ChangeLogRepository _repo = null!;
    private ChangeEvent[] _batch = [];
    private long _seededMaxId;
    private ChangeQuery _lastHour = null!;

    [GlobalSetup]
    public void Setup()
    {
        _dbPath = BenchContext.ScratchDbPath();
        _db = new Db(_dbPath);
        _db.Migrate();
        _repo = new ChangeLogRepository(_db);

        // 100,000 paths × two kinds, spread over the last day, every one distinct, so nothing folds.
        var model = BenchContext.FromEnvironment().Model;
        var files = model.Files;
        var seed = new List<ChangeEvent>(SeedRows);
        for (var i = 0; i < SeedRows; i++)
        {
            var file = files[i % files.Count];
            var kind = i < SeedRows / 2 ? ChangeKind.Modified : ChangeKind.Created;
            seed.Add(new ChangeEvent(file.PathKey, file.PathKey, false, file.Hidden, kind, null, Now.AddSeconds(-i)));
        }

        foreach (var chunk in seed.Chunk(20_000)) _repo.Record(chunk);
        _seededMaxId = _repo.Stamp().MaxId;

        var batch = new List<ChangeEvent>(NewEvents + RepeatEvents);
        for (var i = 0; i < NewEvents; i++)
        {
            var path = $@"Q:\BENCH\CHANGES\NEW{i:0000}.TXT";
            batch.Add(new ChangeEvent(path, path, false, false, ChangeKind.Modified, null, Now.AddSeconds(i)));
        }
        for (var i = 0; i < RepeatEvents; i++)
        {
            var path = $@"Q:\BENCH\CHANGES\NEW{i:0000}.TXT";
            batch.Add(new ChangeEvent(path, path, false, false, ChangeKind.Modified, null, Now.AddSeconds(i + 1)));
        }
        _batch = batch.ToArray();

        _lastHour = new ChangeQuery(Now.AddHours(-1), null, AllKinds, IncludeHidden: false, Limit: 500);
    }

    [IterationSetup(Target = nameof(Record5k))]
    public void DropRecorded()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM fs_change WHERE id > @max;";
        cmd.Parameters.AddWithValue("@max", _seededMaxId);
        cmd.ExecuteNonQuery();
    }

    [Benchmark]
    public void Record5k() => _repo.Record(_batch);

    /// <summary>The "What changed" window's default page: the last hour, newest first, 500 rows.</summary>
    [Benchmark]
    public (IReadOnlyList<ChangeRow> Rows, bool Truncated) QueryLastHour() => _repo.Query(_lastHour);

    /// <summary>The periodic prune with nothing old enough to remove — the scan it pays regardless.</summary>
    [Benchmark]
    public void PruneNothing() => _repo.Prune(Now, TimeSpan.FromDays(3650), SeedRows * 2);

    [GlobalCleanup]
    public void Cleanup()
    {
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}
