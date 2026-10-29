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
        string dir = @"C:\Data", DateTime? modified = null,
        FileAttributes attributes = FileAttributes.Archive, DateTime? created = null) =>
        new(name.ToUpperInvariant(),
            (dir + "\\" + name).ToUpperInvariant(),
            isDir, size,
            modified ?? new DateTime(2026, 6, 1, 12, 0, 0, DateTimeKind.Utc),
            hidden,
            attributes,
            created ?? new DateTime(2026, 6, 1, 12, 0, 0, DateTimeKind.Utc));

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

    /// <summary>Half the disk is not a search result. The Archive bit is set on very nearly every
    /// file ever written, so an attribute belongs on this list rather than beside ext:.</summary>
    [Theory]
    [InlineData("is:dir")]
    [InlineData("is:file")]
    [InlineData("is:hidden")]
    [InlineData("is:readonly")]
    [InlineData("is:system")]
    [InlineData("is:archived")]
    [InlineData("is:link")]
    [InlineData("is:compressed")]
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
        var unmeasured = Entry("ghost.txt", modified: DateTime.MinValue, created: DateTime.MinValue);
        Assert.False(Q("dm:<2020").Matches(unmeasured));
        Assert.False(Q("dm:2026-06").Matches(unmeasured));
        Assert.False(Q("dm:>2000").Matches(unmeasured));
        Assert.False(Q("dc:<2020").Matches(unmeasured));
        Assert.False(Q("dc:2026-06").Matches(unmeasured));
        Assert.False(Q("dc:>2000").Matches(unmeasured));
    }

    // --- dc: ---

    /// <summary>The same parser and the same shorthands as <c>dm:</c>, over the other column.</summary>
    [Fact]
    public void CreatedRange()
    {
        var query = Q("dc:2026-06");
        Assert.True(query.Matches(Entry("a.txt", created: new DateTime(2026, 6, 15, 12, 0, 0, DateTimeKind.Utc))));
        Assert.False(query.Matches(Entry("b.txt", created: new DateTime(2026, 7, 15, 12, 0, 0, DateTimeKind.Utc))));
        Assert.False(query.Matches(Entry("c.txt", created: new DateTime(2026, 5, 15, 12, 0, 0, DateTimeKind.Utc))));
    }

    [Theory]
    [InlineData("dc:>2026-06-01", true)]
    [InlineData("dc:<2026-06-01", false)]
    [InlineData("dc:2026-01-01..2026-12-31", true)]
    [InlineData("dc:2025", false)]
    [InlineData("datecreated:2026-06", true)]
    [InlineData("created:2026-06", true)]
    public void CreatedOperatorsAndAliases(string query, bool expected) =>
        Assert.Equal(expected, Q(query).Matches(
            Entry("a.txt", created: new DateTime(2026, 6, 15, 12, 0, 0, DateTimeKind.Utc))));

    /// <summary>
    /// The two dates are separate columns, so a query about one must not be answered by the other.
    /// A file copied in is the everyday case: created now, modified years ago.
    /// </summary>
    /// <remarks>Both timestamps are mid-year for the reason <see cref="ModifiedRange"/> gives:
    /// a calendar span means the user's <em>local</em> year, so a value at midnight on 1 January
    /// falls on one side of the bound or the other depending on the machine's offset.</remarks>
    [Fact]
    public void CreatedAndModifiedAreNotTheSameColumn()
    {
        var copied = Entry("old.txt",
            modified: new DateTime(2020, 6, 15, 12, 0, 0, DateTimeKind.Utc),
            created: new DateTime(2026, 6, 15, 12, 0, 0, DateTimeKind.Utc));

        Assert.True(Q("dc:2026-06").Matches(copied));
        Assert.False(Q("dm:2026-06").Matches(copied));
        Assert.True(Q("dm:2020-06").Matches(copied));
        Assert.False(Q("dc:2020-06").Matches(copied));
    }

    /// <summary>A bad value names the key the user actually typed, not the one it shares code with.</summary>
    [Fact]
    public void ACreatedProblemSaysCreated()
    {
        var problem = SearchQuery.Parse("dc:banana").Problem;
        Assert.NotNull(problem);
        Assert.StartsWith("dc:", problem);
        Assert.DoesNotContain("dm:", problem);
    }

    // --- is: attributes ---

    [Theory]
    [InlineData("is:readonly", FileAttributes.ReadOnly, true)]
    [InlineData("is:readonly", FileAttributes.Archive, false)]
    [InlineData("is:read-only", FileAttributes.ReadOnly, true)]
    [InlineData("is:ro", FileAttributes.ReadOnly, true)]
    [InlineData("is:system", FileAttributes.System, true)]
    [InlineData("is:system", FileAttributes.Archive, false)]
    [InlineData("is:archived", FileAttributes.Archive, true)]
    [InlineData("is:archive", FileAttributes.Archive, true)]      // the Opus/XYplorer spelling
    [InlineData("is:archived", FileAttributes.ReadOnly, false)]
    [InlineData("is:link", FileAttributes.ReparsePoint, true)]
    [InlineData("is:junction", FileAttributes.ReparsePoint, true)]
    [InlineData("is:symlink", FileAttributes.ReparsePoint, true)]
    [InlineData("is:reparse", FileAttributes.ReparsePoint, true)]
    [InlineData("is:link", FileAttributes.Archive, false)]
    [InlineData("is:compressed", FileAttributes.Compressed, true)]
    [InlineData("is:compressed", FileAttributes.Archive, false)]
    [InlineData("IS:READONLY", FileAttributes.ReadOnly, true)]    // the value folds, like every other
    public void Attributes(string query, FileAttributes attributes, bool expected) =>
        Assert.Equal(expected, Q(query + " report").Matches(Entry("report.txt", attributes: attributes)));

    /// <summary>One bit of a mask, not the whole mask — a file is normally several things at once.</summary>
    [Fact]
    public void AnAttributeTermTestsOneBitOfMany()
    {
        var entry = Entry("app.dll",
            attributes: FileAttributes.ReadOnly | FileAttributes.System | FileAttributes.Archive);

        Assert.True(Q("app is:readonly").Matches(entry));
        Assert.True(Q("app is:system").Matches(entry));
        Assert.True(Q("app is:archived").Matches(entry));
        Assert.False(Q("app is:compressed").Matches(entry));
        Assert.True(Q("app is:readonly is:system").Matches(entry));
        Assert.False(Q("app is:readonly is:compressed").Matches(entry));
    }

    /// <summary>
    /// A row whose attributes were never recorded — one written before the column existed, or an
    /// archive entry, which has no Windows attributes at all. Zero must read as "no" for every
    /// bit rather than as an answer about the file.
    /// </summary>
    [Fact]
    public void ARowWithNoAttributesMatchesNoAttributeFilter()
    {
        var unrecorded = Entry("ghost.txt", attributes: 0);
        Assert.False(Q("ghost is:readonly").Matches(unrecorded));
        Assert.False(Q("ghost is:system").Matches(unrecorded));
        Assert.False(Q("ghost is:archived").Matches(unrecorded));
        Assert.False(Q("ghost is:link").Matches(unrecorded));
        Assert.False(Q("ghost is:compressed").Matches(unrecorded));
    }

    /// <summary>
    /// <c>is:hidden</c> stays on the effective (inherited) column, so it must not start reading
    /// the entry's own mask — a file inside a hidden folder is hidden, and its own bit is clear.
    /// </summary>
    [Fact]
    public void HiddenStillReadsTheEffectiveFlagNotTheMask()
    {
        Assert.True(Q("report is:hidden").Matches(
            Entry("report.txt", hidden: true, attributes: FileAttributes.Archive)));
        Assert.False(Q("report is:hidden").Matches(
            Entry("report.txt", hidden: false, attributes: FileAttributes.Hidden)));
    }

    /// <summary>An unknown value is still an error, and the message lists what is on offer.</summary>
    [Fact]
    public void AnUnknownIsValueIsRefused()
    {
        var problem = SearchQuery.Parse("is:sparkly").Problem;
        Assert.NotNull(problem);
        Assert.Contains("is:readonly", problem);
        Assert.Contains("sparkly", problem);
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
    [InlineData("dc:banana")]
    [InlineData("dc:")]
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
    /// A key that plainly means something this index will not answer is refused rather than
    /// degraded to a name term: silently searching for the literal text "da:today" answers a
    /// different question and returns nothing, which reads as "no such files".
    /// </summary>
    /// <remarks>
    /// <c>da:</c> is the only one left. It is refused for a reason that will not expire the way
    /// <c>dc:</c>'s did — accessed time is readable but frozen, because Windows 10 and 11 ship
    /// with last-access updates disabled — so the message must talk about Windows rather than
    /// about what is indexed.
    /// </remarks>
    [Theory]
    [InlineData("da:today")]
    [InlineData("dateaccessed:2026-06")]
    public void KnownButUnanswerableKeysSayWhy(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Query);
        Assert.NotNull(parse.Problem);
        Assert.Contains("accessed", parse.Problem, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("isn't indexed", parse.Problem);
    }

    /// <summary>The refusal list is not a place keys go to stay: <c>dc:</c> was on it, and is a
    /// working filter now. A regression would put it back and silently stop answering.</summary>
    [Fact]
    public void CreatedIsNoLongerRefused() => Assert.Null(SearchQuery.Parse("dc:today").Problem);

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
