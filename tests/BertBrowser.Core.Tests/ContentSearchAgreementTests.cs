using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The whole content pipeline against an independently computed answer, over a real tree.
/// </summary>
/// <remarks>
/// <para>This is the analogue of <c>SearchAgreementTests</c>, and it exists because that test
/// structurally cannot cover a content term. There, both sides run with no content read: the
/// matcher returns the superset directly, and the SQL side gets <c>1</c> and is re-checked by the
/// <em>same</em> superset matcher. They agree by construction — and they would still agree if
/// <c>ContentTerm</c> answered <see cref="SearchMatch.No"/> for an unread file, because then both
/// come back empty. The existing comparison has no power over this term at all.</para>
/// <para>So the comparison here is different in kind: query goes through the real
/// <see cref="SearchService"/>, and the expected answer is computed by reading every file with
/// <c>File.ReadAllText</c> and applying the same predicate by hand. A first pass that is one row
/// too narrow — a cap in the wrong place, a candidate dropped before it was read — shows up as a
/// missing file here and nowhere else.</para>
/// <para>Both branches of <see cref="SearchService.SearchAsync"/> are covered, and that matters:
/// this runs unindexed, so it takes the <c>FileSystemWalker</c> path exactly as an unelevated app
/// does, and then the same queries run again once the tree has been crawled into <c>fs_entry</c>.
/// Wire the content pass into only one branch and half of these go red.</para>
/// </remarks>
public sealed class ContentSearchAgreementTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _root;
    private readonly FsIndexRepository _repo;
    private readonly IndexCrawler _crawler;
    private readonly SearchService _service;

    /// <summary>Name → contents. Deliberately adversarial about names versus contents.</summary>
    private static readonly (string Path, string Body)[] Corpus =
    [
        ("notes.txt", "the annual report is late\nsecond line\nTODO tidy this"),
        ("decoy.txt", "annual sales figures\nand a separate report"),
        ("code.cs", "// TODO: fix\npublic class Thing { }"),
        ("clean.cs", "public class Other { }"),
        ("sub/deep.md", "nothing interesting\nTODO nested"),
        ("sub/plain.md", "just prose about an annual report"),
        // The needle is in the *name* only: a content search must never match on this, which is
        // what proves the reader ran rather than the name term quietly answering.
        ("TODO.txt", "this file is named for it but does not say the word"),
        ("MixedCase.txt", "Annual Report in title case"),
    ];

    public ContentSearchAgreementTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bb-cagree-{id}.db");
        _root = Path.Combine(Path.GetTempPath(), $"bb-cagree-tree-{id}");

        foreach (var (relative, body) in Corpus)
        {
            var full = Path.Combine(_root, relative.Replace('/', Path.DirectorySeparatorChar));
            Directory.CreateDirectory(Path.GetDirectoryName(full)!);
            File.WriteAllText(full, body);
        }

        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
        _crawler = new IndexCrawler(_repo);
        _service = new SearchService(_repo, _crawler, new NoWatchers(), new NullMftIndexService());
    }

    public void Dispose()
    {
        // Best effort, deliberately. Every live-scan query starts a single-flight background crawl
        // on the *service* lifetime, and disposing does not wait for it — so the database can still
        // be open a moment after the test's assertions have all passed. Throwing here would fail a
        // test that had already succeeded, which is exactly what it did before this catch existed.
        _service.Dispose();
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        try
        {
            foreach (var f in Directory.GetFiles(
                         Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
                File.Delete(f);
            if (Directory.Exists(_root)) Directory.Delete(_root, recursive: true);
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }

    public static TheoryData<string> Queries() =>
    [
        "content:TODO",
        "content:todo",
        "content:\"annual report\"",
        "content:annual",
        "content:TODO ext:cs",
        "content:TODO ext:md",
        "content:annual !content:sales",
        "content:TODO OR content:annual",
        "ext:cs !content:TODO",
        "notes content:annual",
        "content:TODO OR ext:md",
        "content:nothingatallmatchesthis",
        "is:file content:TODO",
    ];

    /// <summary>The answer, computed without going anywhere near the search stack.</summary>
    private IEnumerable<string> BruteForce(string queryText)
    {
        var query = SearchQuery.Parse(queryText).Query!;

        foreach (var (relative, _) in Corpus)
        {
            var full = Path.Combine(_root, relative.Replace('/', Path.DirectorySeparatorChar));
            var text = File.ReadAllText(full);

            var candidate = new SearchCandidate(
                Path.GetFileName(full).ToUpperInvariant(),
                BertBrowser.Core.Paths.PathKey.Canonicalize(full),
                IsDirectory: false,
                SizeBytes: new FileInfo(full).Length,
                ModifiedUtc: File.GetLastWriteTimeUtc(full),
                Hidden: false,
                Content: new ContentText(text, false));

            if (query.Evaluate(candidate) == SearchMatch.Yes)
                yield return Path.GetFileName(full);
        }
    }

    private async Task<IEnumerable<string>> ThroughTheServiceAsync(string queryText)
    {
        var outcome = await _service.SearchAsync(_root, queryText, CancellationToken.None);
        Assert.NotNull(outcome);
        Assert.Null(outcome!.Problem);
        return outcome.Hits.Where(h => !h.IsDirectory).Select(h => h.Name);
    }

    [Theory]
    [MemberData(nameof(Queries))]
    public async Task TheLiveScanBranchAgreesWithReadingEveryFile(string queryText)
    {
        // Unindexed, so this takes FileSystemWalker — the path an unelevated app really uses.
        Assert.Equal(
            BruteForce(queryText).OrderBy(x => x, StringComparer.Ordinal),
            (await ThroughTheServiceAsync(queryText)).OrderBy(x => x, StringComparer.Ordinal));
    }

    [Theory]
    [MemberData(nameof(Queries))]
    public async Task TheIndexBranchAgreesTooOnceTheTreeIsCrawled(string queryText)
    {
        await _crawler.CrawlAsync(_root, CancellationToken.None);

        Assert.Equal(
            BruteForce(queryText).OrderBy(x => x, StringComparer.Ordinal),
            (await ThroughTheServiceAsync(queryText)).OrderBy(x => x, StringComparer.Ordinal));
    }

    /// <summary>
    /// A meta-test: the comparison above has to be able to fail, or a green run means nothing.
    /// </summary>
    [Fact]
    public void TheComparisonNoticesAMissingFile()
    {
        var expected = BruteForce("content:TODO").ToList();
        Assert.NotEmpty(expected);

        var short_ = expected.Skip(1);
        Assert.NotEqual(expected, short_);
    }

    /// <summary>
    /// The assertion that proves the reader ran at all: nothing in these results is explicable by
    /// the name. <c>TODO.txt</c> is named for the needle and does not contain it; the files that
    /// do contain it are not named for it.
    /// </summary>
    [Fact]
    public async Task AContentSearchMatchesOnContentsAndNotOnNames()
    {
        var names = (await ThroughTheServiceAsync("content:TODO")).ToList();

        Assert.DoesNotContain("TODO.txt", names);
        Assert.Contains("notes.txt", names);
        Assert.Contains("code.cs", names);
        Assert.Contains("deep.md", names);
    }

    [Fact]
    public async Task EveryContentHitCarriesTheLineItMatchedOn()
    {
        var outcome = await _service.SearchAsync(_root, "content:TODO", CancellationToken.None);

        var hit = Assert.Single(outcome!.Hits, h => h.Name == "code.cs");
        Assert.NotNull(hit.Match);
        Assert.Equal(1, hit.Match!.LineNumber);
        Assert.Equal("// TODO: fix", hit.Match.Line);
    }

    [Fact]
    public async Task AnOrdinarySearchReportsNoContentScanAtAll()
    {
        // Null is how the caller knows not to show the Match column, so it must stay null for
        // every query that never read a file.
        var outcome = await _service.SearchAsync(_root, "notes", CancellationToken.None);
        Assert.Null(outcome!.ContentScan);

        var content = await _service.SearchAsync(_root, "content:TODO", CancellationToken.None);
        Assert.NotNull(content!.ContentScan);
    }

    private sealed class NoWatchers : IIndexWatcherService
    {
        public bool IsWatching(string rootKey) => false;
        public void Watch(string rootKey, string displayPath) { }
        public void StopAll() { }
        public void Dispose() { }
    }
}
