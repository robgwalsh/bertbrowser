using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The <c>content:</c> term and the three-valued logic it exists for.
/// </summary>
/// <remarks>
/// <para>Everything here is about one question: what does a query say about a file nobody has
/// opened yet? A boolean cannot answer it. Saying "no" drops every real hit before the reader
/// runs; saying "yes" is a superset, but a lossy one that cannot tell a settled hit from a file
/// still to be read — and the whole cost argument for the feature is that those two are different.
/// </para>
/// <para>So the tests below are mostly about <see cref="SearchMatch.NeedsContent"/> travelling
/// correctly through AND, OR and NOT. Collapse it to either boolean in any of the three and
/// something in here goes red.</para>
/// </remarks>
public sealed class ContentTermTests
{
    private static SearchQuery Q(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Problem);
        Assert.NotNull(parse.Query);
        return parse.Query!;
    }

    private static SearchCandidate File(string name, string? content = null) =>
        new(name.ToUpperInvariant(), (@"C:\Data\" + name).ToUpperInvariant(),
            IsDirectory: false, SizeBytes: 100, ModifiedUtc: new DateTime(2026, 6, 1),
            Hidden: false, Content: content is null ? null : new ContentText(content, false));

    private static SearchCandidate Dir(string name) =>
        new(name.ToUpperInvariant(), (@"C:\Data\" + name).ToUpperInvariant(),
            IsDirectory: true, SizeBytes: 0, ModifiedUtc: new DateTime(2026, 6, 1),
            Hidden: false);

    // --- the term itself ---

    [Fact]
    public void AnUnreadFileIsUndecidedRatherThanAMissOrAHit()
    {
        // The load-bearing case. Answer No here and the first pass yields nothing for every
        // content query, so the reader is never handed a candidate and the feature is inert.
        Assert.Equal(SearchMatch.NeedsContent, Q("content:todo").Evaluate(File("a.cs")));
    }

    [Fact]
    public void AnUnreadFileStillCountsAsAPossibleMatchForTheShortlist()
    {
        // SearchQuery.Matches is what FsIndexRepository, the live scan and the archive scanner
        // all call, and it has to keep meaning "could this be a hit?" or phase one drops rows.
        Assert.True(Q("content:todo").Matches(File("a.cs")));
    }

    [Theory]
    [InlineData("hello TODO world", true)]
    [InlineData("hello todo world", true)]   // the needle keeps its case and is compared ignoring it
    [InlineData("nothing to see", false)]
    [InlineData("", false)]
    public void AReadFileIsSettledEitherWay(string content, bool expected)
    {
        var verdict = Q("content:TODO").Evaluate(File("a.cs", content));
        Assert.Equal(expected ? SearchMatch.Yes : SearchMatch.No, verdict);
    }

    [Fact]
    public void TheNeedleKeepsTheCaseItWasTypedIn()
    {
        // Every other value-taking key uppercases in the grammar; this one must not, because the
        // other side of the comparison is a megabyte of file text that would then have to be
        // folded per file, per thread. Fold it here and this goes red — which matters even though
        // matching is case-insensitive either way, because the snippet highlights what was typed.
        Assert.Equal(["MixedCase"], Q("content:MixedCase").ContentNeedles);
    }

    [Fact]
    public void EveryNeedleIsCollectedWhereverItSitsInTheTree()
    {
        Assert.Equal(
            ["alpha", "beta", "gamma"],
            Q("(content:alpha OR content:beta) !content:gamma").ContentNeedles);
    }

    [Fact]
    public void ADirectoryIsSettledNoWithoutEverBeingRead()
    {
        // Not "undecided": a folder has no contents, and leaving it undecided would hand every
        // directory in the tree to a file opener.
        Assert.Equal(SearchMatch.No, Q("content:todo").Evaluate(Dir("sub")));
    }

    // --- AND ---

    [Fact]
    public void OneSettledNoRefusesTheWholeConjunctionWithoutReading()
    {
        // is:dir content:x must cost nothing at all. Let AndNode return NeedsContent here and the
        // scanner opens every directory it is given.
        Assert.Equal(SearchMatch.No, Q("is:dir content:todo").Evaluate(Dir("sub")));
        Assert.Equal(SearchMatch.No, Q("is:dir content:todo").Evaluate(File("a.cs")));
    }

    [Fact]
    public void AConjunctionIsUndecidedOnlyWhenEverythingElseAgrees()
    {
        Assert.Equal(SearchMatch.NeedsContent, Q("report content:todo").Evaluate(File("report.cs")));
        Assert.Equal(SearchMatch.No, Q("report content:todo").Evaluate(File("other.cs")));
    }

    // --- OR ---

    [Fact]
    public void OneSettledYesSatisfiesTheDisjunctionWithoutReading()
    {
        // The other half of the budget argument: in "content:a OR ext:md" every .md is already a
        // hit, and re-establishing that by opening the file would spend the file ceiling on
        // candidates whose names had settled them.
        Assert.Equal(SearchMatch.Yes, Q("content:todo OR ext:md").Evaluate(File("notes.md")));
    }

    [Fact]
    public void ADisjunctionWithNothingSettledIsStillUndecided()
    {
        Assert.Equal(SearchMatch.NeedsContent, Q("content:todo OR ext:md").Evaluate(File("code.cs")));
    }

    [Fact]
    public void ADisjunctionSettlesNoOnlyWhenEveryBranchDoes()
    {
        Assert.Equal(SearchMatch.No, Q("content:todo OR ext:md").Evaluate(File("code.cs", "clean")));
    }

    // --- NOT ---

    [Fact]
    public void NegationLeavesAnUndecidedChildUndecided()
    {
        // The counterpart of "a superset cannot be negated" on the SQL side. Map NeedsContent to
        // No and !content:x returns nothing; map it to Yes and every file is reported without the
        // reader ever being asked.
        Assert.Equal(SearchMatch.NeedsContent, Q("report !content:todo").Evaluate(File("report.cs")));
    }

    [Fact]
    public void NegationFlipsASettledChild()
    {
        Assert.Equal(SearchMatch.No, Q("report !content:todo").Evaluate(File("report.cs", "a TODO here")));
        Assert.Equal(SearchMatch.Yes, Q("report !content:todo").Evaluate(File("report.cs", "clean")));
    }

    [Fact]
    public void TwoContentTermsBothHaveToBeSatisfied()
    {
        var q = Q("content:alpha content:beta");
        Assert.Equal(SearchMatch.NeedsContent, q.Evaluate(File("a.cs")));
        Assert.Equal(SearchMatch.Yes, q.Evaluate(File("a.cs", "alpha and beta")));
        Assert.Equal(SearchMatch.No, q.Evaluate(File("a.cs", "alpha only")));
    }

    // --- the query-level flag ---

    [Theory]
    [InlineData("content:todo", true)]
    [InlineData("report content:todo", true)]
    [InlineData("content:todo OR report", true)]
    [InlineData("report !content:todo", true)]   // deliberately unlike WantsHidden/WantsArchives
    [InlineData("report", false)]
    [InlineData("ext:md size:>1kb", false)]
    public void NeedsContentIsWhatDecidesWhetherTheSecondPassRuns(string text, bool expected) =>
        Assert.Equal(expected, Q(text).NeedsContent);

    /// <summary>
    /// <c>!content:x</c> propagates where <c>!is:hidden</c> and <c>!in:archives</c> do not, and
    /// this is the test that pins the asymmetry. Those two ask for the default rather than for
    /// more work; establishing that a file does <em>not</em> contain something needs it read
    /// exactly as much as establishing that it does.
    /// </summary>
    [Fact]
    public void NegationPropagatesNeedsContentButNotTheOtherTwoScopes()
    {
        Assert.True(Q("report !content:todo").NeedsContent);
        Assert.False(Q("report !is:hidden").WantsHidden);
        Assert.False(Q("report !in:archives").WantsArchives);
    }

    // --- the SQL face ---

    [Fact]
    public void AContentTermCompilesToAnIncompletePredicate()
    {
        // No column holds file text, so the widest possible predicate is the only honest one --
        // and saying it is incomplete is what stops LIMIT being pushed down past it.
        var predicate = Q("content:todo").Compile();
        Assert.False(predicate.Complete);
    }

    [Fact]
    public void NegatingAContentTermDoesNotInvertItsSql()
    {
        // NOT 1 would return nothing at all. The existing NotNode rule handles this once
        // SqlComplete says the child is a superset.
        var predicate = Q("report !content:todo").Compile();
        Assert.False(predicate.Complete);
        Assert.DoesNotContain("NOT", predicate.Sql);
    }
}
