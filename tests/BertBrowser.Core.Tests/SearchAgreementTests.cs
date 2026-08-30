using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The test that makes the two faces of a query impossible to drift apart.
/// </summary>
/// <remarks>
/// <para>Every query runs twice over one corpus: through SQLite via
/// <see cref="FsIndexRepository.Search"/>, and directly through
/// <see cref="SearchQuery.Matches"/>. The names must be identical.</para>
/// <para>This is what stands behind the design in <c>SearchNode</c> — the SQL is only an
/// optimisation over the matcher, so a <c>WriteSql</c> that is too wide is free and one that
/// is too narrow is a bug. It is also the regression guard for the failure that would
/// otherwise be invisible: an indexed drive and an unindexed one answering the same query
/// differently, because only one of them went through SQL.</para>
/// </remarks>
public sealed class SearchAgreementTests : IDisposable
{
    private readonly string _dbPath;
    private readonly FsIndexRepository _repo;

    private const string Root = @"C:\Corpus";

    /// <summary>Fixed, so a failure names the same file every time.</summary>
    private static readonly DateTime Base = new(2026, 6, 15, 12, 0, 0, DateTimeKind.Utc);

    private static readonly (string Path, bool IsDir, long Size, DateTime Modified, bool Hidden)[] Corpus =
    {
        (@"C:\Corpus\report.txt",                 false, 1024,          Base,                  false),
        (@"C:\Corpus\report.docx",                false, 2_000_000,     Base.AddDays(-1),      false),
        (@"C:\Corpus\Report-DRAFT.txt",           false, 10,            Base.AddDays(-40),     false),
        (@"C:\Corpus\notes.txt",                  false, 0,             Base.AddYears(-2),     false),
        (@"C:\Corpus\photo.jpg",                  false, 5_000_000,     Base,                  false),
        (@"C:\Corpus\photo.jpeg",                 false, 4_000_000,     Base,                  false),
        (@"C:\Corpus\IMG_0042.jpg",               false, 3_000_000,     Base.AddDays(-3),      false),
        (@"C:\Corpus\IMG_7.png",                  false, 900,           Base,                  false),
        (@"C:\Corpus\.gitignore",                 false, 40,            Base,                  true),
        (@"C:\Corpus\a[1].txt",                   false, 12,            Base,                  false),
        (@"C:\Corpus\a*b.txt",                    false, 13,            Base,                  false),
        (@"C:\Corpus\Übung-01.pdf",               false, 700_000,       Base,                  false),
        (@"C:\Corpus\Projects",                   true,  0,             Base,                  false),
        (@"C:\Corpus\Projects\alpha.txt",         false, 1_048_576,     Base.AddDays(-10),     false),
        (@"C:\Corpus\Projects\beta.log",          false, 20_000_000,    Base.AddDays(-200),    false),
        (@"C:\Corpus\Projects\gamma.log",         false, 100,           Base,                  true),
        (@"C:\Corpus\Projects\report final.txt",  false, 4096,          Base,                  false),
        (@"C:\Corpus\Archive",                    true,  0,             Base,                  false),
        (@"C:\Corpus\Archive\report.txt",         false, 512,           Base.AddYears(-3),     false),
        (@"C:\Corpus\Archive\ghost.bin",          false, 0,             DateTime.MinValue,     false),
    };

    /// <summary>Queries chosen to exercise every term type, and every way of combining them.</summary>
    public static TheoryData<string> Queries()
    {
        var data = new TheoryData<string>();
        foreach (var q in new[]
                 {
                     "report", "rep ort", "REPORT", "übung", "*.txt", "?eport", "a[1]",
                     "\"report final\"", "\"a*b\"",
                     "ext:txt", "ext:jpg", "ext:jpg;png", "ext:log ext:log",
                     "size:>1mb", "size:<1kb", "size:>=1024", "size:<=1024",
                     "size:1kb..2mb", "size:empty", "size:=100",
                     "dm:2026-06", "dm:>2026-06-01", "dm:<2025-01-01", "dm:2024..2026",
                     "is:dir report", "is:file report", "report is:hidden",
                     "path:projects", "path:archive report",
                     "re:^report", @"re:^img_\d+", "re:log$",
                     "report !draft", "!draft ext:txt", "report !ext:docx",
                     "report OR photo", "ext:jpg OR ext:png", "(report OR photo) ext:txt",
                     "report !re:draft", "!re:^img ext:jpg",
                     "report ext:txt size:>100 dm:>2020",
                     "path:corpus !path:archive ext:txt",
                 })
            data.Add(q);
        return data;
    }

    public SearchAgreementTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-agree-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);

        _repo.UpsertEntries(
            Corpus.Select(e => new FsEntryRow(
                PathKey.Canonicalize(e.Path), Path.GetFileName(e.Path.TrimEnd('\\')),
                e.IsDir, e.Size, e.Modified, e.Hidden)).ToList(),
            crawlGen: 1);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    private static SearchCandidate Candidate(
        (string Path, bool IsDir, long Size, DateTime Modified, bool Hidden) e) =>
        new(Path.GetFileName(e.Path.TrimEnd('\\')).ToUpperInvariant(),
            PathKey.Canonicalize(e.Path), e.IsDir, e.Size, e.Modified, e.Hidden);

    [Theory]
    [MemberData(nameof(Queries))]
    public void SqlAndTheMatcherReturnTheSameEntries(string text)
    {
        var query = SearchQuery.Parse(text).Query;
        Assert.NotNull(query);

        var fromSql = _repo.Search(Root, query!, cap: 1000, includeHidden: true)
            .Hits.Select(h => h.DisplayPath.ToUpperInvariant()).OrderBy(x => x, StringComparer.Ordinal);

        var fromMatcher = Corpus
            .Where(e => query!.Matches(Candidate(e)))
            .Select(e => PathKey.Canonicalize(e.Path))
            .OrderBy(x => x, StringComparer.Ordinal);

        Assert.Equal(fromMatcher, fromSql);
    }

    /// <summary>
    /// Meta-test: the comparison above can actually fail. Without this, a bug that made both
    /// sides return nothing would look like agreement.
    /// </summary>
    [Fact]
    public void TheComparisonNoticesADisagreement()
    {
        var query = SearchQuery.Parse("report").Query!;

        var fromSql = _repo.Search(Root, query, cap: 1000, includeHidden: true)
            .Hits.Select(h => h.DisplayPath.ToUpperInvariant()).OrderBy(x => x, StringComparer.Ordinal)
            .ToList();

        Assert.NotEmpty(fromSql);
        Assert.NotEqual(fromSql, fromSql.Skip(1).ToList());
    }

    /// <summary>
    /// A regex compiles to a superset, so the row set SQLite returns is filtered again in C#.
    /// If that re-check were dropped, this query would return every entry in the subtree.
    /// </summary>
    [Fact]
    public void AnIncompletePredicateStillReturnsOnlyRealMatches()
    {
        var query = SearchQuery.Parse(@"re:^img_\d+").Query!;
        var hits = _repo.Search(Root, query, cap: 1000, includeHidden: true).Hits;

        Assert.Equal(
            new[] { "IMG_0042.jpg", "IMG_7.png" },
            hits.Select(h => h.Name).OrderBy(n => n, StringComparer.Ordinal));
    }

    /// <summary>
    /// Rows with a length are what tells a measured volume from one built by the names-only
    /// fallback, where every row lands at size 0 and a size filter can never match. Recognised
    /// the way <c>DiskUsageRules</c> recognises it: rows, but not one length among them.
    /// </summary>
    [Fact]
    public void SizeDataIsDetectedPerScope()
    {
        Assert.True(_repo.HasSizeData(Root));
        Assert.True(_repo.HasSizeData(null));

        // Archive holds a 512-byte file and a zero-byte one, so it is measured...
        Assert.True(_repo.HasSizeData(@"C:\Corpus\Archive"));
        // ...whereas a subtree of nothing but sizeless rows looks exactly like the fallback build.
        Assert.False(_repo.HasSizeData(@"C:\Corpus\Nowhere"));
    }

    /// <summary>
    /// The cap counts rows that matched, not rows the scan happened to read — which is why
    /// LIMIT is withheld from an incomplete predicate.
    /// </summary>
    [Fact]
    public void TheCapCountsMatchesEvenWhenThePredicateIsASuperset()
    {
        var query = SearchQuery.Parse("re:txt$").Query!;
        var (hits, truncated) = _repo.Search(Root, query, cap: 2, includeHidden: true);

        Assert.Equal(2, hits.Count);
        Assert.True(truncated);
        Assert.All(hits, h => Assert.EndsWith(".txt", h.Name, StringComparison.OrdinalIgnoreCase));
    }
}
