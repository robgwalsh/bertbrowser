using System.Text;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Search;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchiveSearchTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-archsearch-{Guid.NewGuid():N}");

    private readonly SharpCompressArchiveReader _reader = new();
    private readonly string _zip;

    public ArchiveSearchTests()
    {
        Directory.CreateDirectory(_root);
        _zip = Zip("a.zip",
            ("readme.txt", "hello"),
            ("src/app.js", new string('x', 500)),
            ("src/lib/util.js", "util"),
            ("docs/guide.md", "guide"),
            ("docs/notes.txt", "notes"));
    }

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Zip(string name, params (string Key, string Body)[] entries)
    {
        var path = Path.Combine(_root, name);
        using var file = File.Create(path);
        using var writer = WriterFactory.Open(file, ArchiveType.Zip, new WriterOptions(CompressionType.Deflate));
        foreach (var (key, body) in entries)
        {
            using var source = new MemoryStream(Encoding.UTF8.GetBytes(body));
            writer.Write(key, source, new DateTime(2026, 2, 3, 4, 5, 6));
        }
        return path;
    }

    private IReadOnlyList<string> Find(string queryText, string relativeTo = "")
    {
        var parse = SearchQuery.Parse(queryText);
        Assert.NotNull(parse.Query);

        return ArchiveSearchScanner
            .Search(_reader.Read(_zip, null), _zip, relativeTo, parse.Query!, 1000)
            .Select(h => h.Name)
            .Order()
            .ToList();
    }

    [Fact]
    public void APlainWordMatchesEntryNames()
    {
        Assert.Equal(["util.js"], Find("util"));
    }

    /// <summary>
    /// The strongest evidence the shape is right: the scanner reuses SearchNode.Matches verbatim,
    /// so every filter the box understands works inside a container without being reimplemented.
    /// </summary>
    [Theory]
    [InlineData("ext:js", new[] { "app.js", "util.js" })]
    [InlineData("ext:md;txt", new[] { "guide.md", "notes.txt", "readme.txt" })]
    // A size bound excludes directories, here as everywhere else. Folders inside a container do
    // carry an exact recursive total, so this *could* have been special-cased — but the term
    // means the same thing wherever it is typed, and one filter behaving differently inside an
    // archive is a worse surprise than a folder it does not reach.
    [InlineData("size:>100", new[] { "app.js" })]
    [InlineData("re:^app", new[] { "app.js" })]
    [InlineData("notes OR guide", new[] { "guide.md", "notes.txt" })]
    [InlineData("ext:txt !readme", new[] { "notes.txt" })]
    // is:dir carries no filter of its own — "every folder" is not a search — so it is paired with
    // one that matches any name, exactly as it would have to be in the box.
    [InlineData("is:dir re:.", new[] { "docs", "lib", "src" })]
    public void EveryFilterTheBoxUnderstandsWorksInHere(string query, string[] expected)
    {
        Assert.Equal(expected, Find(query));
    }

    [Fact]
    public void SearchingFromInsideAFolderOnlyLooksBelowIt()
    {
        Assert.Equal(["guide.md", "notes.txt"], Find("ext:md;txt", "docs"));
        Assert.DoesNotContain("readme.txt", Find("ext:md;txt", "docs"));
    }

    /// <summary>
    /// A container gives no timestamp for a folder it never listed explicitly, and the date terms
    /// already apply a 1601 floor — so such an entry satisfies no date filter rather than every
    /// open-ended one.
    /// </summary>
    [Fact]
    public void AnEntryWithNoTimestampSatisfiesNoDateFilter()
    {
        // "src" is synthesized from a path prefix and carries no date of its own.
        Assert.DoesNotContain("src", Find("dm:2000-01..2100-01"));
    }

    [Fact]
    public void HitsCarryVirtualPathsAndTheirFolderColumn()
    {
        var parse = SearchQuery.Parse("util");
        var hits = ArchiveSearchScanner.Search(_reader.Read(_zip, null), _zip, "", parse.Query!, 1000);

        var hit = Assert.Single(hits);
        Assert.Equal(Path.Combine(_zip, "src", "lib", "util.js"), hit.DisplayPath);
        Assert.Equal(@"src\lib", hit.RelativeDirDisplay);
    }

    [Fact]
    public void TheLimitStopsTheWalk()
    {
        var parse = SearchQuery.Parse("ext:js;md;txt");
        var hits = ArchiveSearchScanner.Search(_reader.Read(_zip, null), _zip, "", parse.Query!, 2);

        Assert.Equal(2, hits.Count);
    }

    [Fact]
    public void ADamagedArchiveYieldsNothingRatherThanThrowing()
    {
        var broken = Path.Combine(_root, "broken.zip");
        File.WriteAllText(broken, "not a zip");

        var parse = SearchQuery.Parse("anything");
        var hits = ArchiveSearchScanner.Search(
            _reader.Read(broken, null), broken, "", parse.Query!, 100);

        Assert.Empty(hits);
    }

    // --- the in:archives scope ---

    // Paired with something, because a scope on its own is not a search — see the floor rule below.
    [Theory]
    [InlineData("report in:archives")]
    [InlineData("report in:archive")]
    [InlineData("ext:txt in:archives")]
    [InlineData("(a OR b) in:archives")]
    public void InArchivesAsksForTheSecondPass(string queryText)
    {
        var parse = SearchQuery.Parse(queryText);

        Assert.NotNull(parse.Query);
        Assert.True(parse.Query!.WantsArchives);
    }

    /// <summary>
    /// An exclusion asks to leave containers out, which is already what happens — widening the scan
    /// would be the opposite of what was typed. The same rule <c>!is:hidden</c> follows.
    /// </summary>
    [Fact]
    public void NotInArchivesDoesNotWidenTheScan()
    {
        var parse = SearchQuery.Parse("report !in:archives");

        Assert.NotNull(parse.Query);
        Assert.False(parse.Query!.WantsArchives);
    }

    [Fact]
    public void AnOrdinaryQueryNeverAsksForIt()
    {
        Assert.False(SearchQuery.Parse("report ext:txt").Query!.WantsArchives);
    }

    [Fact]
    public void AnUnknownScopeIsAProblemRatherThanANameTerm()
    {
        var parse = SearchQuery.Parse("in:everything");

        Assert.Null(parse.Query);
        Assert.NotNull(parse.Problem);
        Assert.Contains("in:archives", parse.Problem!);
    }

    /// <summary>
    /// The marker matches everything and emits <c>1</c>, so on its own it is not a search — the
    /// floor rules must still refuse it, or typing the scope alone would list the whole disk.
    /// </summary>
    [Fact]
    public void TheScopeAloneIsNotASearch()
    {
        Assert.Null(SearchQuery.Parse("in:archives").Query);
    }

    /// <summary>The card cannot advertise a filter the parser does not implement.</summary>
    [Fact]
    public void TheSyntaxCardAdvertisesIt()
    {
        Assert.Contains(SearchSyntax.Entries, e => e.Example.Contains("in:archives"));
    }
}
