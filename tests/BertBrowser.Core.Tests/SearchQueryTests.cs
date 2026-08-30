using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What a bare query has always meant. Every theory here predates the filter syntax and is kept
/// verbatim in spirit: the point of the file is that adding <c>ext:</c> and friends did not
/// quietly change what an ordinary search does.
/// </summary>
public sealed class SearchQueryTests
{
    private static SearchQuery? Q(string? text) => SearchQuery.Parse(text).Query;

    /// <summary>An entry as the matcher sees it. Name keys are uppercased by the callers —
    /// the walker folds on its way to a path key, the index stores name_key folded.</summary>
    private static SearchCandidate File(string name, long size = 0, bool isDir = false) =>
        new(name.ToUpperInvariant(), (@"C:\Data\" + name).ToUpperInvariant(),
            isDir, size, new DateTime(2026, 6, 1, 12, 0, 0, DateTimeKind.Utc), false);

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("a")]          // one literal char: too broad
    [InlineData("a*")]         // wildcards don't count as literal chars
    [InlineData("* ?")]
    public void Parse_RejectsEmptyOrTooBroad(string? text) => Assert.Null(Q(text));

    [Theory]
    [InlineData("ab")]
    [InlineData(".c")]         // "*.c" style still has two literal chars with the dot
    [InlineData("a b")]        // two single-char terms: two literal chars total
    public void Parse_AcceptsTwoLiteralChars(string text) => Assert.NotNull(Q(text));

    [Fact]
    public void Parse_UppercasesAndSplitsOnWhitespace()
    {
        var query = Q("  proj\treport ")!;
        Assert.True(query.Matches(File("Project-Report-2026.docx")));
        Assert.False(query.Matches(File("Project-Plan.docx")));
    }

    [Fact]
    public void Compile_WrapsInStarsAndEscapesOpenBracket()
    {
        Assert.Contains("*A[[]1]B*", Q("a[1]b")!.Compile().Parameters.Select(p => (string)p.Value));
        Assert.Contains("*REPORT*", Q("report")!.Compile().Parameters.Select(p => (string)p.Value));
    }

    [Theory]
    [InlineData("rep", "Report.docx", true)]                   // substring, case-insensitive
    [InlineData("rep", "preparation.txt", true)]
    [InlineData("rep", "notes.txt", false)]
    [InlineData("proj rep", "Project-Report-2026.docx", true)] // AND terms
    [InlineData("proj rep", "Project-Plan.docx", false)]
    [InlineData("*.txt", "notes.txt", true)]                   // explicit star
    [InlineData("*.txt", "notes.txt.bak", true)]               // substring semantics: still contains ".txt"
    [InlineData("?eport", "Report.docx", true)]                // single-char wildcard
    [InlineData("x?z", "xyz-file.bin", true)]
    [InlineData("x?z", "xz-file.bin", false)]
    [InlineData("a[1]", "a[1].txt", true)]                     // literal brackets
    public void Matches_SubstringWildcardsAndAnd(string queryText, string name, bool expected) =>
        Assert.Equal(expected, Q(queryText)!.Matches(File(name)));

    [Fact]
    public void Matches_FoldsNonAsciiLikeToUpperInvariant()
    {
        // SQLite NOCASE would miss this — our folding must not.
        Assert.True(Q("übung")!.Matches(File("Übung-01.pdf")));
    }

    // --- What the filter syntax must NOT have taken away ---

    [Theory]
    [InlineData(@"C:\Users")]   // a pasted path: 'c' is not a filter key
    [InlineData("time:30")]     // an unknown key is ordinary text
    [InlineData("ratio 16:9")]
    public void AnUnrecognisedKeyStaysALiteralNameTerm(string text)
    {
        var query = Q(text);
        Assert.NotNull(query);
        // It searches for the text as typed, colon and all.
        Assert.True(query!.Matches(File(text.Replace(@"C:\", "C-") + ".txt"))
                    || query.Matches(File(text + ".txt")));
    }

    [Fact]
    public void LowercaseOrIsAWordNotAnOperator()
    {
        // A file really called "Report or Draft" has to stay findable, and no existing query
        // may change meaning because a new keyword appeared.
        var query = Q("report or draft")!;
        Assert.True(query.Matches(File("Report or Draft.txt")));
        Assert.False(query.Matches(File("Report.txt")));
    }

    [Fact]
    public void UppercaseOrIsAnOperator()
    {
        var query = Q("report OR draft")!;
        Assert.True(query.Matches(File("Report.txt")));
        Assert.True(query.Matches(File("Draft.txt")));
        Assert.False(query.Matches(File("Notes.txt")));
    }

    [Theory]
    [InlineData("report!")]     // trailing '!' is a character, not an operator
    [InlineData("hello!!")]
    public void ATrailingExclamationMarkIsLiteral(string text) =>
        Assert.True(Q(text)!.Matches(File(text + ".txt")));

    [Fact]
    public void AQuotedPhraseKeepsItsSpacesAndDropsWildcards()
    {
        var query = Q("\"annual report\"")!;
        Assert.True(query.Matches(File("The annual report.docx")));
        Assert.False(query.Matches(File("annual-report.docx")));

        var stars = Q("\"a*b\"")!;
        Assert.True(stars.Matches(File("a*b.txt")));
        Assert.False(stars.Matches(File("axxb.txt")));
    }
}
