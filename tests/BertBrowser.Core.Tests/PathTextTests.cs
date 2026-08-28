using BertBrowser.Core.Paths;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class PathTextTests
{
    /// <summary>Explorer quotes every path, space or no space, and so does this — a result that is
    /// only sometimes paste-safe is worse than one that always is.</summary>
    [Theory]
    [InlineData(@"C:\Program Files\app.exe")]
    [InlineData(@"C:\tools\app.exe")]
    public void EveryPathIsQuoted(string path) =>
        Assert.Equal($"\"{path}\"", PathText.Quote(path));

    [Fact]
    public void SeveralPathsAreOnePerLine()
    {
        var text = PathText.ForClipboard([@"C:\a.txt", @"C:\b b.txt"]);

        Assert.Equal($@"""C:\a.txt""{Environment.NewLine}""C:\b b.txt""", text);
    }

    [Fact]
    public void NamesAreBare()
    {
        var text = PathText.NamesForClipboard([@"C:\dir\a.txt", @"C:\dir\sub"]);

        Assert.Equal($"a.txt{Environment.NewLine}sub", text);
    }

    [Fact]
    public void NothingSelectedIsEmptyRatherThanAStrayNewline()
    {
        Assert.Equal("", PathText.ForClipboard([]));
        Assert.Equal("", PathText.NamesForClipboard([]));
    }
}
