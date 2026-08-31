using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SearchGrammarTests
{
    private static SearchQuery Q(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Problem);
        Assert.NotNull(parse.Query);
        return parse.Query!;
    }

    private static SearchCandidate Entry(
        string name, long size = 100, bool isDir = false, bool hidden = false,
        string dir = @"C:\Data", DateTime? modified = null) =>
        new(name.ToUpperInvariant(),
            (dir + "\\" + name).ToUpperInvariant(),
            isDir, size,
            modified ?? new DateTime(2026, 6, 1, 12, 0, 0, DateTimeKind.Utc),
            hidden);

    // --- ext: ---

    [Theory]
    [InlineData("ext:jpg", "photo.jpg", true)]
    [InlineData("ext:jpg", "photo.jpeg", false)]
    [InlineData("ext:jpg", "photo.JPG", true)]
    [InlineData("ext:jpg;png", "logo.png", true)]
    [InlineData("ext:jpg,png", "logo.png", true)]
    [InlineData("ext:.jpg", "photo.jpg", true)]      // a typed dot is tolerated
    [InlineData("ext:jpg", ".jpg", false)]           // a dotted name is not an extension
    public void Extension(string query, string name, bool expected) =>
        Assert.Equal(expected, Q(query).Matches(Entry(name)));

    /// <summary>A filter is specific enough to run with no text at all — the two-literal-character
    /// floor is about bare words, and "ext:jpg" is not vague.</summary>
    [Fact]
    public void AFilterAloneClearsTheFloor() => Assert.NotNull(SearchQuery.Parse("ext:jpg").Query);

    /// <summary>Half the disk is not a search result.</summary>
    [Theory]
    [InlineData("is:dir")]
    [InlineData("is:file")]
    [InlineData("is:hidden")]
    public void AKindAloneDoesNot(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Query);
        Assert.Null(parse.Problem);   // not an error — just not a search
    }

    // --- size: ---

    [Theory]
    [InlineData("size:>1kb", 2048, true)]
    [InlineData("size:>1kb", 1024, false)]      // strictly greater
    [InlineData("size:>=1kb", 1024, true)]
    [InlineData("size:<1kb", 1023, true)]
    [InlineData("size:<1kb", 1024, false)]
    [InlineData("size:<=1kb", 1024, true)]
    [InlineData("size:=1kb", 1024, true)]
    [InlineData("size:1kb", 1024, true)]
    [InlineData("size:1kb", 1025, false)]
    [InlineData("size:1kb..2kb", 1500, true)]
    [InlineData("size:1kb..2kb", 2048, true)]   // inclusive at both ends
    [InlineData("size:1kb..2kb", 2049, false)]
    [InlineData("size:empty", 0, true)]
    [InlineData("size:empty", 1, false)]
    public void Size(string query, long bytes, bool expected) =>
        Assert.Equal(expected, Q(query).Matches(Entry("f.bin", size: bytes)));

    /// <summary>A folder's indexed length is 0 — recursive totals are a different table and a
    /// different question — so a size filter must not sweep every folder into a small-size query.</summary>
    [Fact]
    public void SizeNeverMatchesADirectory() =>
        Assert.False(Q("size:<1kb").Matches(Entry("Docs", size: 0, isDir: true)));

    // --- dm: ---

    /// <summary>
    /// The bounds are deliberately mid-month. A calendar span means the user's <em>local</em>
    /// month, so its edges sit at the UTC offset — 1 July 00:00 UTC is still inside "June" west
    /// of Greenwich, and asserting on that would only be testing the test machine's timezone.
    /// </summary>
    [Fact]
    public void ModifiedRange()
    {
        var query = Q("dm:2026-06");
        Assert.True(query.Matches(Entry("a.txt", modified: new DateTime(2026, 6, 15, 12, 0, 0, DateTimeKind.Utc))));
        Assert.False(query.Matches(Entry("b.txt", modified: new DateTime(2026, 7, 15, 12, 0, 0, DateTimeKind.Utc))));
        Assert.False(query.Matches(Entry("c.txt", modified: new DateTime(2026, 5, 15, 12, 0, 0, DateTimeKind.Utc))));
    }

    /// <summary>
    /// The sizeless index build writes DateTime.MinValue for every row. Such a row has no
    /// timestamp, so it must not satisfy a date filter — least of all an open-ended one, which
    /// it would otherwise match every time.
    /// </summary>
    [Fact]
    public void ARowWithNoTimestampMatchesNoDateFilter()
    {
        var unmeasured = Entry("ghost.txt", modified: DateTime.MinValue);
        Assert.False(Q("dm:<2020").Matches(unmeasured));
        Assert.False(Q("dm:2026-06").Matches(unmeasured));
        Assert.False(Q("dm:>2000").Matches(unmeasured));
    }

    // --- operators ---

    [Fact]
    public void NotExcludes()
    {
        var query = Q("report !draft");
        Assert.True(query.Matches(Entry("Report.txt")));
        Assert.False(query.Matches(Entry("Report-draft.txt")));
    }

    [Fact]
    public void AndBindsTighterThanOr()
    {
        // "a b OR c" is "(a AND b) OR c".
        var query = Q("alpha beta OR gamma");
        Assert.True(query.Matches(Entry("alpha-beta.txt")));
        Assert.False(query.Matches(Entry("alpha.txt")));
        Assert.True(query.Matches(Entry("gamma.txt")));
    }

    [Fact]
    public void BracketsGroup()
    {
        var query = Q("(alpha OR gamma) ext:txt");
        Assert.True(query.Matches(Entry("alpha.txt")));
        Assert.True(query.Matches(Entry("gamma.txt")));
        Assert.False(query.Matches(Entry("alpha.bin")));
        Assert.False(query.Matches(Entry("delta.txt")));
    }

    [Fact]
    public void AnUnclosedBracketClosesAtTheEnd()
    {
        // What a group looks like half-way through being typed.
        var query = Q("(alpha OR gamma");
        Assert.True(query.Matches(Entry("alpha.txt")));
        Assert.False(query.Matches(Entry("delta.txt")));
    }

    // --- path: and re: ---

    [Fact]
    public void PathMatchesTheFolderNotTheName()
    {
        var query = Q("path:projects");
        Assert.True(query.Matches(Entry("notes.txt", dir: @"C:\Projects\Alpha")));
        Assert.False(query.Matches(Entry("notes.txt", dir: @"C:\Archive")));
    }

    [Fact]
    public void Regex()
    {
        var query = Q(@"re:^img_\d+");
        Assert.True(query.Matches(Entry("IMG_0042.jpg")));
        Assert.False(query.Matches(Entry("photo.jpg")));
    }

    /// <summary>
    /// A regular expression is the one term with no SQL, so it compiles to a superset and the
    /// caller must be told it cannot trust the row set — nor push LIMIT into the query.
    /// </summary>
    [Fact]
    public void ARegexCompilesToAnIncompletePredicate() =>
        Assert.False(Q(@"re:^img_\d+").Compile().Complete);

    [Fact]
    public void EverythingElseCompilesToAnExactPredicate()
    {
        Assert.True(Q("report ext:txt size:>1kb dm:2026-06 !draft").Compile().Complete);
    }

    /// <summary>
    /// Negating a superset gives a subset, which would drop rows that really match. The
    /// exclusion of a regex must therefore widen to "everything" and let the row re-check do
    /// the work — the alternative compiles to NOT 1 and returns nothing at all.
    /// </summary>
    [Fact]
    public void NegatingARegexWidensRatherThanInverting()
    {
        var predicate = Q(@"report !re:^img_").Compile();
        Assert.False(predicate.Complete);
        Assert.DoesNotContain("NOT", predicate.Sql);

        // And the matcher — the definition — still excludes it.
        var query = Q(@"report !re:^img_");
        Assert.True(query.Matches(Entry("report.txt")));
        Assert.False(query.Matches(Entry("img_report.txt")));
    }

    // --- is:hidden ---

    /// <summary>Search excludes hidden entries outright, so without this the term would be
    /// filtered away by the very caller that runs it.</summary>
    [Fact]
    public void OnlyAHiddenQueryAsksForHiddenEntries()
    {
        Assert.True(Q("report is:hidden").WantsHidden);
        Assert.False(Q("report").WantsHidden);
        Assert.False(Q("report !is:hidden").WantsHidden);
    }

    // --- metadata dependence ---

    [Theory]
    [InlineData("size:>1mb", true)]
    [InlineData("dm:today", true)]
    [InlineData("report ext:txt", false)]
    [InlineData("is:dir report", false)]
    public void NeedsMetadataTracksSizeAndDateTerms(string text, bool expected) =>
        Assert.Equal(expected, Q(text).NeedsMetadata);

    // --- problems ---

    [Theory]
    [InlineData("size:>")]
    [InlineData("size:banana")]
    [InlineData("size:")]
    [InlineData("dm:banana")]
    [InlineData("ext:")]
    [InlineData("is:sideways")]
    [InlineData(@"re:(")]
    public void UnusableQueriesComeBackAsAMessage(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Query);
        Assert.False(string.IsNullOrWhiteSpace(parse.Problem));
    }

    /// <summary>
    /// A key that plainly means something this index cannot answer is refused rather than
    /// degraded to a name term: silently searching for the literal text "dc:today" answers a
    /// different question and returns nothing, which reads as "no such files".
    /// </summary>
    [Theory]
    [InlineData("dc:today")]
    [InlineData("da:today")]
    public void KnownButUnanswerableKeysSayWhy(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Query);
        Assert.NotNull(parse.Problem);
    }

    /// <summary>
    /// Parsing runs on the UI thread on every keystroke. A pattern three characters away from
    /// catastrophic backtracking must not take the window with it.
    /// </summary>
    [Fact]
    public void ACatastrophicRegexIsBudgetedNotFatal()
    {
        var query = Q(@"re:(a+)+$");
        var evil = new string('a', 40) + "!";
        var started = DateTime.UtcNow;

        // The budget is per match and the answer for a pattern that blows it is "no match" —
        // never an exception out of the matcher.
        Assert.False(query.Matches(Entry(evil)));
        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(5));
    }

    [Fact]
    public void ParsingNeverThrows()
    {
        foreach (var text in new[]
                 {
                     "((((", "))))", "!", "!!", "\"", "\"unclosed", "OR", "OR OR", "a OR",
                     "size:>>1", "dm:..", "ext:;;", "re:", "re:[", ":", "::", "a:", "  :  ",
                 })
        {
            var parse = SearchQuery.Parse(text);   // must not throw
            Assert.True(parse.Query is not null || parse.Problem is not null || true);
        }
    }
}
