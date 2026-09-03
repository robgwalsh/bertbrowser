using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Changes;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChangeLogRepositoryTests : IDisposable
{
    private static readonly DateTime T0 = new(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);
    private static readonly IReadOnlySet<ChangeKind> AllKinds =
        new HashSet<ChangeKind> { ChangeKind.Created, ChangeKind.Modified, ChangeKind.Deleted, ChangeKind.Renamed };

    private readonly string _dbPath;
    private readonly ChangeLogRepository _repo;

    public ChangeLogRepositoryTests()
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

    private static ChangeEvent Ev(string displayPath, ChangeKind kind, DateTime utc,
        bool isDir = false, bool hidden = false, string? oldPath = null) =>
        new(PathKey.Canonicalize(displayPath), displayPath, isDir, hidden, kind, oldPath, utc);

    private static ChangeQuery Q(DateTime since, string? scope = null, IReadOnlySet<ChangeKind>? kinds = null,
        bool includeHidden = false, int limit = 100) =>
        new(since, scope is null ? null : PathKey.Canonicalize(scope), kinds ?? AllKinds, includeHidden, limit);

    [Fact]
    public void Record_CoalescesRepeatedWritesInsideTheWindow()
    {
        _repo.Record([
            Ev(@"C:\Logs\app.log", ChangeKind.Modified, T0),
            Ev(@"C:\Logs\app.log", ChangeKind.Modified, T0.AddSeconds(30)),
            Ev(@"C:\Logs\app.log", ChangeKind.Modified, T0.AddSeconds(59)),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));

        var row = Assert.Single(rows);
        Assert.Equal(3, row.Count);
        Assert.Equal(T0, row.FirstUtc);
        Assert.Equal(T0.AddSeconds(59), row.LastUtc);
    }

    [Fact]
    public void Record_StartsANewRowOnceTheWindowHasPassed()
    {
        _repo.Record([Ev(@"C:\Logs\app.log", ChangeKind.Modified, T0)]);
        _repo.Record([Ev(@"C:\Logs\app.log", ChangeKind.Modified, T0.AddSeconds(61))]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));

        Assert.Equal(2, rows.Count);
        Assert.All(rows, r => Assert.Equal(1, r.Count));
    }

    [Fact]
    public void Record_DoesNotFoldDifferentKindsOfTheSamePath()
    {
        _repo.Record([
            Ev(@"C:\Tmp\setup.tmp", ChangeKind.Created, T0),
            Ev(@"C:\Tmp\setup.tmp", ChangeKind.Deleted, T0.AddSeconds(5)),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));

        Assert.Equal(2, rows.Count);
    }

    [Fact]
    public void Record_KeepsTheFirstRowsPathsWhenFolding()
    {
        _repo.Record([
            Ev(@"C:\Down\installer.msi", ChangeKind.Renamed, T0, oldPath: @"C:\Down\installer.part"),
            Ev(@"C:\Down\installer.msi", ChangeKind.Renamed, T0.AddSeconds(10), oldPath: @"C:\Down\other.part"),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));

        var row = Assert.Single(rows);
        Assert.Equal(@"C:\Down\installer.part", row.OldDisplayPath);
        Assert.Equal(@"C:\Down\installer.msi", row.DisplayPath);
        Assert.Equal(ChangeKind.Renamed, row.Kind);
    }

    [Fact]
    public void Record_EmptyBatchTouchesNothing()
    {
        _repo.Record([]);

        Assert.Equal(0, _repo.Count());
    }

    [Fact]
    public void Query_ScopesToTheSubtreeAndNotItsNeighbours()
    {
        _repo.Record([
            Ev(@"C:\Foo\a.txt", ChangeKind.Created, T0),
            Ev(@"C:\Foo\Sub\b.txt", ChangeKind.Created, T0),
            Ev(@"C:\Foobar\c.txt", ChangeKind.Created, T0),
            Ev(@"D:\Foo\d.txt", ChangeKind.Created, T0),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1), scope: @"C:\Foo"));

        Assert.Equal(2, rows.Count);
        Assert.All(rows, r => Assert.StartsWith(@"C:\FOO\", r.PathKey));
    }

    [Fact]
    public void Query_FiltersByKind()
    {
        _repo.Record([
            Ev(@"C:\a.txt", ChangeKind.Created, T0),
            Ev(@"C:\b.txt", ChangeKind.Modified, T0),
            Ev(@"C:\c.txt", ChangeKind.Deleted, T0),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1), kinds: new HashSet<ChangeKind> { ChangeKind.Created, ChangeKind.Deleted }));

        Assert.Equal(2, rows.Count);
        Assert.DoesNotContain(rows, r => r.Kind == ChangeKind.Modified);
    }

    [Fact]
    public void Query_HidesHiddenUnlessAsked()
    {
        _repo.Record([
            Ev(@"C:\visible.txt", ChangeKind.Created, T0),
            Ev(@"C:\.cache\index", ChangeKind.Created, T0, hidden: true),
        ]);

        var (without, _) = _repo.Query(Q(T0.AddHours(-1)));
        var (with, _) = _repo.Query(Q(T0.AddHours(-1), includeHidden: true));

        Assert.Single(without);
        Assert.Equal(2, with.Count);
    }

    [Fact]
    public void Query_IsNewestFirstAndBoundedBelow()
    {
        _repo.Record([
            Ev(@"C:\old.txt", ChangeKind.Created, T0.AddHours(-5)),
            Ev(@"C:\mid.txt", ChangeKind.Created, T0.AddMinutes(-30)),
            Ev(@"C:\new.txt", ChangeKind.Created, T0),
        ]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));

        Assert.Equal([@"C:\new.txt", @"C:\mid.txt"], rows.Select(r => r.DisplayPath));
    }

    [Fact]
    public void Query_ReportsTruncationPastTheLimit()
    {
        _repo.Record(Enumerable.Range(0, 5).Select(i => Ev($@"C:\f{i}.txt", ChangeKind.Created, T0.AddSeconds(i))).ToList());

        var (rows, truncated) = _repo.Query(Q(T0.AddHours(-1), limit: 3));

        Assert.Equal(3, rows.Count);
        Assert.True(truncated);
    }

    [Fact]
    public void Query_NeedsNoSorter()
    {
        // ORDER BY last_utc DESC must come off ix_fs_change_last, whatever else is filtered: a temp
        // B-tree here would mean materialising every row in range before the LIMIT could stop it.
        _repo.Record([Ev(@"C:\Foo\a.txt", ChangeKind.Created, T0)]);

        foreach (var query in new[]
                 {
                     Q(T0.AddHours(-1)),
                     Q(T0.AddHours(-1), scope: @"C:\Foo"),
                     Q(T0.AddHours(-1), scope: @"C:\Foo", kinds: new HashSet<ChangeKind> { ChangeKind.Deleted }),
                 })
        {
            var plan = string.Join("\n", _repo.ExplainQuery(query));
            Assert.DoesNotContain("TEMP B-TREE", plan);
            Assert.Contains("ix_fs_change_last", plan);
        }
    }

    [Fact]
    public void Prune_DropsRowsOlderThanTheRetention()
    {
        _repo.Record([
            Ev(@"C:\old.txt", ChangeKind.Created, T0.AddHours(-30)),
            Ev(@"C:\kept.txt", ChangeKind.Created, T0.AddHours(-2)),
        ]);

        _repo.Prune(T0, TimeSpan.FromHours(24));

        var (rows, _) = _repo.Query(Q(T0.AddDays(-7)));
        Assert.Equal(@"C:\kept.txt", Assert.Single(rows).DisplayPath);
    }

    [Fact]
    public void Prune_CapsTheTableKeepingTheNewest()
    {
        _repo.Record(Enumerable.Range(0, 10).Select(i => Ev($@"C:\f{i}.txt", ChangeKind.Created, T0.AddSeconds(i))).ToList());

        _repo.Prune(T0.AddMinutes(1), TimeSpan.FromHours(24), maxRows: 4);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1)));
        Assert.Equal([@"C:\f9.txt", @"C:\f8.txt", @"C:\f7.txt", @"C:\f6.txt"], rows.Select(r => r.DisplayPath));
    }

    [Fact]
    public void Prune_UnderTheCapIsANoOp()
    {
        _repo.Record([Ev(@"C:\a.txt", ChangeKind.Created, T0)]);

        _repo.Prune(T0, TimeSpan.FromHours(24), maxRows: 4);

        Assert.Equal(1, _repo.Count());
    }

    [Fact]
    public void Clear_EmptiesTheTable()
    {
        _repo.Record([Ev(@"C:\a.txt", ChangeKind.Created, T0), Ev(@"C:\b.txt", ChangeKind.Created, T0)]);

        _repo.Clear();

        Assert.Equal(0, _repo.Count());
    }

    [Fact]
    public void Stamp_MovesWhenARowIsFoldedNotOnlyWhenOneIsAdded()
    {
        var empty = _repo.Stamp();
        _repo.Record([Ev(@"C:\a.txt", ChangeKind.Modified, T0)]);
        var first = _repo.Stamp();
        _repo.Record([Ev(@"C:\a.txt", ChangeKind.Modified, T0.AddSeconds(10))]);
        var folded = _repo.Stamp();

        Assert.NotEqual(empty, first);
        Assert.NotEqual(first, folded);
        Assert.Equal(1, _repo.Count());
    }

    [Fact]
    public void Query_ReadsBackEveryColumn()
    {
        _repo.Record([Ev(@"C:\Proj\bin", ChangeKind.Created, T0, isDir: true, hidden: true)]);

        var (rows, _) = _repo.Query(Q(T0.AddHours(-1), includeHidden: true));

        var row = Assert.Single(rows);
        Assert.Equal(@"C:\PROJ\BIN", row.PathKey);
        Assert.Equal(@"C:\Proj\bin", row.DisplayPath);
        Assert.True(row.IsDirectory);
        Assert.True(row.Hidden);
        Assert.Null(row.OldDisplayPath);
        Assert.Equal(DateTimeKind.Utc, row.LastUtc.Kind);
    }
}
