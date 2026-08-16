using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The elevated helper's command line. Nothing on it is trusted: it arrives from another process,
/// and the helper has an administrator token to lose.
/// </summary>
public class IndexerArgumentsTests
{
    private static IndexerArguments Parse(params string[] args)
    {
        Assert.True(IndexerArguments.TryParse(args, out var result, out var error), error);
        return result;
    }

    private static string Rejects(params string[] args)
    {
        Assert.False(IndexerArguments.TryParse(args, out _, out var error));
        Assert.NotEqual("", error);
        return error;
    }

    [Fact]
    public void ParsesAFullCommandLine()
    {
        var options = Parse("--pipe", "BertBrowser.Index.S-1-5-21-1.abc123", "--parent-pid", "4242",
            "--data-dir", @"C:\Users\Rob\.bertbrowser");

        Assert.Equal("BertBrowser.Index.S-1-5-21-1.abc123", options.PipeName);
        Assert.Equal(4242, options.ParentProcessId);
        Assert.Equal(@"C:\Users\Rob\.bertbrowser", options.DataDirectory);
    }

    [Fact]
    public void ArgumentOrderDoesNotMatter()
    {
        var options = Parse("--data-dir", @"C:\Data", "--parent-pid", "7", "--pipe", "BertBrowser.Index.x");

        Assert.Equal(7, options.ParentProcessId);
        Assert.Equal(@"C:\Data", options.DataDirectory);
    }

    [Theory]
    [InlineData()]
    [InlineData("--pipe", "BertBrowser.Index.x")]
    [InlineData("--pipe", "BertBrowser.Index.x", "--parent-pid", "1")]
    [InlineData("--parent-pid", "1", "--data-dir", @"C:\Data")]
    public void RequiresEveryArgument(params string[] args)
    {
        Rejects(args);
    }

    [Theory]
    [InlineData("--pipe")]
    [InlineData("--parent-pid")]
    [InlineData("--data-dir")]
    public void RejectsAFlagWithNoValue(string flag)
    {
        Rejects(flag);
    }

    /// <summary>
    /// An unrecognised option is an error, never a positional value — the same rule the user-facing
    /// command line follows. A mistyped flag silently becoming a data directory is much worse than
    /// a message.
    /// </summary>
    [Theory]
    [InlineData("--elevate")]
    [InlineData("-p")]
    [InlineData(@"C:\Windows")]
    public void RejectsAnUnrecognisedArgument(string extra)
    {
        Rejects("--pipe", "BertBrowser.Index.x", "--parent-pid", "1", "--data-dir", @"C:\Data", extra);
    }

    [Theory]
    [InlineData("0")]
    [InlineData("-1")]
    [InlineData("nine")]
    [InlineData("")]
    public void RejectsAParentProcessIdThatIsNotPositive(string pid)
    {
        Rejects("--pipe", "BertBrowser.Index.x", "--parent-pid", pid, "--data-dir", @"C:\Data");
    }

    [Theory]
    [InlineData("BertBrowser.Index.S-1-5-21-1.abcdef")]
    [InlineData("BertBrowser.Index.x")]
    public void AcceptsAPipeNameThisAppWouldGenerate(string name)
    {
        Assert.True(IndexerArguments.IsAcceptablePipeName(name));
    }

    /// <summary>
    /// The name becomes a <c>\\.\pipe\</c> path, so a separator in it would name a different object
    /// than the one intended.
    /// </summary>
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("Something.Else")]
    [InlineData(@"BertBrowser.Index.\..\evil")]
    [InlineData("BertBrowser.Index./evil")]
    [InlineData("BertBrowser.Index.a:b")]
    [InlineData("BertBrowser.Index.a*")]
    [InlineData("BertBrowser.Index.a\u0000b")]
    [InlineData("bertbrowser.index.x")]
    public void RejectsAPipeNameItWouldNot(string? name)
    {
        Assert.False(IndexerArguments.IsAcceptablePipeName(name));
    }

    [Fact]
    public void RejectsAnOverlongPipeName()
    {
        var name = "BertBrowser.Index." + new string('a', IndexerArguments.MaxPipeNameLength);

        Assert.False(IndexerArguments.IsAcceptablePipeName(name));
    }

    [Theory]
    [InlineData(@"..\..\Windows")]
    [InlineData(@".bertbrowser")]
    [InlineData(@"\\.\C:")]
    [InlineData(@"C:\Data\*")]
    public void RejectsADataDirectoryThatIsNotAnAcceptableAbsolutePath(string dir)
    {
        Rejects("--pipe", "BertBrowser.Index.x", "--parent-pid", "1", "--data-dir", dir);
    }

    [Fact]
    public void AcceptsAUncDataDirectory()
    {
        var options = Parse("--pipe", "BertBrowser.Index.x", "--parent-pid", "1",
            "--data-dir", @"\\server\share\data");

        Assert.Equal(@"\\server\share\data", options.DataDirectory);
    }
}
