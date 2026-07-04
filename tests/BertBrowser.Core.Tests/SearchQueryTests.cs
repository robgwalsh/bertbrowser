using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SearchQueryTests
{
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("a")]          // one literal char: too broad
    [InlineData("a*")]         // wildcards don't count as literal chars
    [InlineData("* ?")]
    public void Parse_RejectsEmptyOrTooBroad(string? text) =>
        Assert.Null(SearchQuery.Parse(text));

    [Theory]
    [InlineData("ab")]
    [InlineData(".c")]         // "*.c" style still has two literal chars with the dot
    [InlineData("a b")]        // two single-char terms: two literal chars total
    public void Parse_AcceptsTwoLiteralChars(string text) =>
        Assert.NotNull(SearchQuery.Parse(text));

    [Fact]
    public void Parse_UppercasesAndSplitsOnWhitespace()
    {
        var query = SearchQuery.Parse("  proj\treport ")!;
        Assert.Equal(new[] { "PROJ", "REPORT" }, query.Terms);
    }

    [Fact]
    public void GlobPatterns_WrapInStarsAndEscapeOpenBracket()
    {
        var query = SearchQuery.Parse("a[1]b")!;
        Assert.Equal("*A[[]1]B*", query.GlobPatterns.Single());

        var plain = SearchQuery.Parse("report")!;
        Assert.Equal("*REPORT*", plain.GlobPatterns.Single());
    }

    [Theory]
    [InlineData("rep", "Report.docx", true)]                 // substring, case-insensitive
    [InlineData("rep", "preparation.txt", true)]
    [InlineData("rep", "notes.txt", false)]
    [InlineData("proj rep", "Project-Report-2026.docx", true)] // AND terms
    [InlineData("proj rep", "Project-Plan.docx", false)]
    [InlineData("*.txt", "notes.txt", true)]                  // explicit star
    [InlineData("*.txt", "notes.txt.bak", true)]               // substring semantics: still contains ".txt"
    [InlineData("?eport", "Report.docx", true)]                // single-char wildcard
    [InlineData("x?z", "xyz-file.bin", true)]
    [InlineData("x?z", "xz-file.bin", false)]
    [InlineData("a[1]", "a[1].txt", true)]                     // literal brackets
    public void Matches_SubstringWildcardsAndAnd(string queryText, string name, bool expected)
    {
        var query = SearchQuery.Parse(queryText)!;
        Assert.Equal(expected, query.Matches(name));
    }

    [Fact]
    public void Matches_FoldsNonAsciiLikeToUpperInvariant()
    {
        // SQLite NOCASE would miss this — our folding must not.
        var query = SearchQuery.Parse("übung")!;
        Assert.True(query.Matches("Übung-01.pdf"));
    }
}
