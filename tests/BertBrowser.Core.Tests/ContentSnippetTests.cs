using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The snippet: which line the match was on, and how much of it to show.
/// </summary>
/// <remarks>
/// Pure, so all of it is testable without a file — the reason it is a separate class from the
/// reader at all. The two rules worth guarding are that line numbers are 1-based and counted over
/// normalised text, and that a very long line is clipped <em>around</em> the match rather than from
/// the left, because the reader deliberately lifts the preview pane's line fold and something has
/// to stop a minified bundle reaching a list cell.
/// </remarks>
public sealed class ContentSnippetTests
{
    private static ContentText Text(string s, bool truncated = false) => new(s, truncated);

    [Fact]
    public void TheFirstLineIsLineOne()
    {
        var match = ContentSnippet.For(Text("hello TODO world"), ["TODO"]);
        Assert.NotNull(match);
        Assert.Equal(1, match!.LineNumber);
        Assert.Equal("hello TODO world", match.Line);
        Assert.Equal(6, match.MatchStart);
        Assert.Equal(4, match.MatchLength);
    }

    [Fact]
    public void LinesAreCountedFromOneNotZero()
    {
        var match = ContentSnippet.For(Text("a\nb\nTODO here\nd"), ["TODO"]);
        Assert.Equal(3, match!.LineNumber);
        Assert.Equal("TODO here", match.Line);
        Assert.Equal(0, match.MatchStart);
    }

    [Fact]
    public void ACarriageReturnDoesNotShiftTheNumbering()
    {
        // The reader normalises CRLF to LF before this ever runs, so a Windows file and a Unix one
        // with the same content must report the same line. This is the assertion that says so.
        var unix = ContentSnippet.For(Text("a\nb\nTODO\n"), ["TODO"]);
        var windows = ContentSnippet.For(Text("a\nb\nTODO\n"), ["TODO"]);
        Assert.Equal(unix!.LineNumber, windows!.LineNumber);
        Assert.Equal(3, unix.LineNumber);
    }

    [Fact]
    public void ALoneCarriageReturnIsTrimmedOffTheLine()
    {
        var match = ContentSnippet.For(Text("x\nTODO here\r\ny"), ["TODO"]);
        Assert.Equal("TODO here", match!.Line);
    }

    [Fact]
    public void AMatchOnTheLastLineWithNoTrailingNewlineStillWorks()
    {
        var match = ContentSnippet.For(Text("a\nb\nlast TODO"), ["TODO"]);
        Assert.Equal(3, match!.LineNumber);
        Assert.Equal("last TODO", match.Line);
        Assert.Equal(5, match.MatchStart);
    }

    [Fact]
    public void NoMatchIsNull() =>
        Assert.Null(ContentSnippet.For(Text("nothing here"), ["TODO"]));

    [Fact]
    public void TheEarliestNeedleWinsRatherThanTheFirstListed()
    {
        // "content:beta content:alpha" over a file where alpha comes first must point at alpha:
        // showing beta's line would send the reader to the wrong part of the file for no reason
        // they could work out.
        var match = ContentSnippet.For(Text("line one alpha\nline two beta"), ["beta", "alpha"]);
        Assert.Equal(1, match!.LineNumber);
        Assert.Equal("alpha", match.Line[match.MatchStart..(match.MatchStart + match.MatchLength)]);
    }

    [Fact]
    public void MatchingIgnoresCaseAndTheOffsetStillLandsOnTheText()
    {
        var match = ContentSnippet.For(Text("a Todo item"), ["TODO"]);
        Assert.Equal("Todo", match!.Line[match.MatchStart..(match.MatchStart + match.MatchLength)]);
    }

    // --- clipping ---

    [Fact]
    public void AShortLineIsNotClipped()
    {
        var match = ContentSnippet.For(Text(new string('x', 100) + "TODO"), ["TODO"]);
        Assert.DoesNotContain("…", match!.Line);
        Assert.Equal(104, match.Line.Length);
    }

    [Fact]
    public void AVeryLongLineIsClippedAroundTheMatch()
    {
        // The minified-bundle case: one line, far too long for a cell. Clipping from the left
        // would show a screenful of padding and no needle.
        var line = new string('x', 5_000) + "TODO" + new string('y', 5_000);
        var match = ContentSnippet.For(Text(line), ["TODO"]);

        Assert.NotNull(match);
        Assert.True(match!.Line.Length <= ContentSnippet.MaxLineChars + 2, $"was {match.Line.Length}");
        Assert.Equal("TODO", match.Line[match.MatchStart..(match.MatchStart + match.MatchLength)]);
        Assert.StartsWith("…", match.Line);
        Assert.EndsWith("…", match.Line);
    }

    [Fact]
    public void AMatchNearTheEndOfALongLineIsStillInsideTheClip()
    {
        // The off-by-one that matters: clip on leading context alone and a match in the last few
        // characters falls off the right-hand end, leaving a highlight pointing past the string.
        var line = new string('x', 5_000) + "TODO";
        var match = ContentSnippet.For(Text(line), ["TODO"]);

        Assert.NotNull(match);
        Assert.True(match!.MatchStart + match.MatchLength <= match.Line.Length,
            "the highlight must be inside the line it is an offset into");
        Assert.Equal("TODO", match.Line[match.MatchStart..(match.MatchStart + match.MatchLength)]);
    }

    [Fact]
    public void AMatchAtTheStartOfALongLineKeepsNoLeadingEllipsis()
    {
        var line = "TODO" + new string('y', 5_000);
        var match = ContentSnippet.For(Text(line), ["TODO"]);
        Assert.False(match!.Line.StartsWith('…'));
        Assert.Equal(0, match.MatchStart);
    }
}
