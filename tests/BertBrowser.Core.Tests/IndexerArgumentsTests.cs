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
    public void ParsesTheOrdinaryCommandLine()
    {
        var options = Parse("--data-dir", @"C:\Users\Rob\.bertbrowser");

        Assert.Equal(IndexerCommand.Run, options.Command);
        Assert.Equal(@"C:\Users\Rob\.bertbrowser", options.DataDirectory);
    }

    [Theory]
    [InlineData("--register-autostart", IndexerCommand.RegisterAutoStart)]
    [InlineData("--unregister-autostart", IndexerCommand.UnregisterAutoStart)]
    public void ParsesTheAutoStartCommands(string flag, IndexerCommand expected)
    {
        var options = Parse(flag, "--data-dir", @"C:\Data");

        Assert.Equal(expected, options.Command);
        Assert.Equal(@"C:\Data", options.DataDirectory);
    }

    [Fact]
    public void ArgumentOrderDoesNotMatter()
    {
        var options = Parse("--data-dir", @"C:\Data", "--register-autostart");

        Assert.Equal(IndexerCommand.RegisterAutoStart, options.Command);
        Assert.Equal(@"C:\Data", options.DataDirectory);
    }

    [Fact]
    public void RefusesToBothRegisterAndUnregister()
    {
        Rejects("--register-autostart", "--unregister-autostart", "--data-dir", @"C:\Data");
    }

    [Theory]
    [InlineData()]
    [InlineData("--register-autostart")]
    public void RequiresTheDataDirectory(params string[] args)
    {
        Rejects(args);
    }

    [Fact]
    public void RejectsAFlagWithNoValue()
    {
        Rejects("--data-dir");
    }

    /// <summary>
    /// <b>The pipe name and the parent process id are gone, not ignored.</b> The helper derives its
    /// own endpoint and has no parent to watch, so there is nothing for a caller to choose — and an
    /// argument that parsed but did nothing would read as a check still being made.
    /// </summary>
    [Theory]
    [InlineData("--pipe", "BertBrowser.Index.x")]
    [InlineData("--parent-pid", "4242")]
    public void RejectsTheArgumentsItNoLongerTakes(string flag, string value)
    {
        var error = Rejects("--data-dir", @"C:\Data", flag, value);
        Assert.Contains(flag, error, StringComparison.Ordinal);
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
        Rejects("--data-dir", @"C:\Data", extra);
    }

    [Theory]
    [InlineData(@"..\..\Windows")]
    [InlineData(@".bertbrowser")]
    [InlineData(@"\\.\C:")]
    [InlineData(@"C:\Data\*")]
    public void RejectsADataDirectoryThatIsNotAnAcceptableAbsolutePath(string dir)
    {
        Rejects("--data-dir", dir);
    }

    [Fact]
    public void AcceptsAUncDataDirectory()
    {
        var options = Parse("--data-dir", @"\\server\share\data");

        Assert.Equal(@"\\server\share\data", options.DataDirectory);
    }
}
