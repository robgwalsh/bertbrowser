using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Changes;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChangeRecorderTests : IDisposable
{
    private static readonly DateTime T0 = new(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);
    private const string DataDir = @"C:\Users\Rob\.bertbrowser";

    private readonly string _dbPath;
    private readonly ChangeLogRepository _repo;
    private ChangeLogPolicy _policy = ChangeLogPolicy.FromHours(24);
    private DateTime _now = T0;

    public ChangeRecorderTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new ChangeLogRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    private ChangeRecorder Recorder(ChangeLogRepository? repo = null) =>
        new(repo ?? _repo, PathKey.Canonicalize(DataDir), () => _policy, () => _now);

    private static ChangeEvent Ev(string displayPath, DateTime? utc = null) =>
        new(PathKey.Canonicalize(displayPath), displayPath, false, false, ChangeKind.Modified, null, utc ?? T0);

    private long Rows() => _repo.Count();

    [Fact]
    public void Off_WritesNothing()
    {
        _policy = ChangeLogPolicy.Off;
        var recorder = Recorder();

        recorder.Add(Ev(@"C:\Work\notes.txt"));
        recorder.Flush();

        Assert.Equal(0, Rows());
    }

    [Fact]
    public void On_WritesTheBatchOnFlush()
    {
        var recorder = Recorder();

        recorder.Add(Ev(@"C:\Work\a.txt"));
        recorder.Add(Ev(@"C:\Work\b.txt"));
        Assert.Equal(0, Rows());

        recorder.Flush();

        Assert.Equal(2, Rows());
    }

    [Fact]
    public void TurningOff_WipesTheTableOnce()
    {
        var recorder = Recorder();
        recorder.Add(Ev(@"C:\Work\a.txt"));
        recorder.Flush();
        Assert.Equal(1, Rows());

        // The verb arrives between two polls; whatever was buffered meanwhile must not land.
        recorder.Add(Ev(@"C:\Work\b.txt"));
        _policy = ChangeLogPolicy.Off;
        recorder.Flush();
        Assert.Equal(0, Rows());

        // Once. A later flush while off does not keep wiping — the app may be seeding, testing, or
        // simply not have asked.
        _repo.Record([Ev(@"C:\Elsewhere\c.txt")]);
        recorder.Flush();
        Assert.Equal(1, Rows());
    }

    [Fact]
    public void OnThenOffBeforeAnyFlush_LandsNothingAndWipesNothing()
    {
        // The helper came up, was told "on", buffered a batch, and was told "off" before its
        // first poll wrote anything. The batch must not land — and nothing was ever written by
        // this recorder, so there is nothing of its own to wipe; the app owns that wipe.
        var recorder = Recorder();
        recorder.Add(Ev(@"C:\Work\a.txt"));
        _repo.Record([Ev(@"C:\Elsewhere\seeded.txt")]);

        _policy = ChangeLogPolicy.Off;
        recorder.Flush();

        Assert.Equal(1, Rows());
    }

    [Fact]
    public void DataDirectory_IsNeverRecorded()
    {
        var recorder = Recorder();

        recorder.Add(Ev(Path.Combine(DataDir, "bertbrowser.db-wal")));
        recorder.Add(Ev(Path.Combine(DataDir, "settings.json")));
        recorder.Add(Ev(@"C:\Users\Rob\notes.txt"));
        recorder.Flush();

        Assert.Equal(1, Rows());
    }

    [Fact]
    public void EmptyFlush_LeavesTheDatabaseFileUntouched()
    {
        var recorder = Recorder();
        recorder.Add(Ev(@"C:\Work\a.txt"));
        recorder.Flush();
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        var before = Snapshot();

        _now = T0.AddMinutes(5); // a prune would be due, if anything ran at all
        recorder.Flush();
        recorder.Flush();

        Assert.Equal(before, Snapshot());
    }

    [Fact]
    public void Prune_RunsWithTheFirstBatchAndThenOncePerInterval()
    {
        _policy = ChangeLogPolicy.FromHours(1);
        var recorder = Recorder();
        _repo.Record([Ev(@"C:\Old\one.txt", T0.AddHours(-30))]);

        recorder.Add(Ev(@"C:\Work\a.txt", T0));
        recorder.Flush();
        Assert.Equal(1, Rows()); // the 30-hour-old row went with the first flush

        _repo.Record([Ev(@"C:\Old\two.txt", T0.AddHours(-30))]);
        _now = T0.AddSeconds(30);
        recorder.Add(Ev(@"C:\Work\b.txt", _now));
        recorder.Flush();
        Assert.Equal(3, Rows()); // inside the interval: nothing pruned

        _now = T0.AddSeconds(61);
        recorder.Add(Ev(@"C:\Work\c.txt", _now));
        recorder.Flush();
        Assert.Equal(3, Rows()); // a, b, c — the stale row went on the interval
    }

    [Fact]
    public void ARepositoryFailure_DisablesTheRecorderAndNeverThrows()
    {
        // A database the app never migrated: no fs_change table. The tail must survive this — a
        // throw here would silently end the volume's index maintenance.
        var bare = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        try
        {
            var recorder = Recorder(new ChangeLogRepository(new Db(bare)));

            recorder.Add(Ev(@"C:\Work\a.txt"));
            recorder.Flush();

            Assert.True(recorder.IsDisabled);
            recorder.Add(Ev(@"C:\Work\b.txt"));
            recorder.Flush(); // still nothing
        }
        finally
        {
            Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
            foreach (var f in Directory.GetFiles(Path.GetDirectoryName(bare)!, Path.GetFileName(bare) + "*"))
                File.Delete(f);
        }
    }

    /// <summary>Size and write time of the database and its WAL — what any write would move.</summary>
    private (long, DateTime, long, DateTime) Snapshot()
    {
        var wal = _dbPath + "-wal";
        var db = new FileInfo(_dbPath);
        var w = new FileInfo(wal);
        return (db.Length, db.LastWriteTimeUtc, w.Exists ? w.Length : -1, w.Exists ? w.LastWriteTimeUtc : default);
    }
}
