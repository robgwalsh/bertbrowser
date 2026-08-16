using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The wire format an administrator-token process reads. Every theory here is a thing a peer must
/// not be able to say; relax <see cref="IndexProtocol.IsAcceptableRootKey"/> or the verb rule and
/// this file goes red.
/// </summary>
public class IndexProtocolTests
{
    private static IndexMessage Parse(string line)
    {
        Assert.True(IndexProtocol.TryParse(line, out var message), $"expected to parse: {line}");
        return message;
    }

    private static void Rejects(string? line)
    {
        Assert.False(IndexProtocol.TryParse(line, out _));
    }

    [Theory]
    [InlineData(IndexVerb.Ready, "")]
    [InlineData(IndexVerb.Start, "")]
    [InlineData(IndexVerb.Shutdown, "")]
    [InlineData(IndexVerb.Ping, "")]
    [InlineData(IndexVerb.Pong, "")]
    [InlineData(IndexVerb.Hello, "1")]
    [InlineData(IndexVerb.Building, "C")]
    [InlineData(IndexVerb.Idle, "D")]
    [InlineData(IndexVerb.Complete, @"C:\")]
    [InlineData(IndexVerb.Fatal, "The database schema is newer than this helper.")]
    public void EveryVerbRoundTrips(IndexVerb verb, string argument)
    {
        var message = new IndexMessage(verb, argument);

        var parsed = Parse(IndexProtocol.Format(message));

        Assert.Equal(verb, parsed.Verb);
        Assert.Equal(argument, parsed.Argument);
    }

    [Fact]
    public void FormatsAsATabSeparatedPrefixedLine()
    {
        Assert.Equal("IDX\tComplete\tC:\\", IndexProtocol.Format(new IndexMessage(IndexVerb.Complete, @"C:\")));
        Assert.Equal("IDX\tStart", IndexProtocol.Format(new IndexMessage(IndexVerb.Start)));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("Start")]
    [InlineData("OPEN\tStart")]
    [InlineData("idx\tStart")]
    [InlineData("IDX")]
    [InlineData(" IDX\tStart")]
    public void RejectsAnythingWithoutTheRightPrefix(string? line)
    {
        Rejects(line);
    }

    [Theory]
    [InlineData("IDX\tReindex\tC:\\")]
    [InlineData("IDX\tDelete\tC:\\")]
    [InlineData("IDX\tstart")]
    [InlineData("IDX\tSTART")]
    [InlineData("IDX\t")]
    public void RejectsAnUnknownVerb(string line)
    {
        Rejects(line);
    }

    /// <summary>
    /// <c>Enum.TryParse</c> accepts a bare number and a comma-separated list as well as a name.
    /// Neither is a verb anyone meant to send, and both would land on a real one.
    /// </summary>
    [Theory]
    [InlineData("IDX\t2")]
    [InlineData("IDX\t0")]
    [InlineData("IDX\tStart,Shutdown")]
    [InlineData("IDX\t Start")]
    public void RejectsANumericOrCompositeVerb(string line)
    {
        Rejects(line);
    }

    [Fact]
    public void RejectsMoreFieldsThanAVerbAndOneArgument()
    {
        Rejects("IDX\tComplete\tC:\\\tD:\\");
    }

    [Fact]
    public void RejectsAnOversizedLine()
    {
        var line = "IDX\tFatal\t" + new string('a', NavigationRequest.MaxLineLength);

        Rejects(line);
    }

    [Theory]
    [InlineData("C")]
    [InlineData("Z")]
    public void AcceptsABareDriveLetter(string drive)
    {
        Assert.True(IndexProtocol.IsAcceptableDrive(drive));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("c")]
    [InlineData("C:")]
    [InlineData("C:\\")]
    [InlineData("CD")]
    [InlineData("1")]
    [InlineData("\\")]
    public void RejectsAnythingThatIsNotABareDriveLetter(string? drive)
    {
        Assert.False(IndexProtocol.IsAcceptableDrive(drive));
    }

    [Theory]
    [InlineData("IDX\tBuilding\tC:")]
    [InlineData("IDX\tBuilding\tc")]
    [InlineData("IDX\tBuilding")]
    [InlineData("IDX\tIdle\tC:\\WINDOWS")]
    public void RejectsABuildingOrIdleThatIsNotADriveLetter(string line)
    {
        Rejects(line);
    }

    [Theory]
    [InlineData(@"C:\")]
    [InlineData(@"Z:\")]
    public void AcceptsACanonicalVolumeRoot(string root)
    {
        Assert.True(IndexProtocol.IsAcceptableRootKey(root));
    }

    /// <summary>
    /// The rule the elevated end's state feeds: anything accepted here becomes a root the search
    /// router treats as fully indexed. Deeper paths, relative paths, device paths and UNC shares
    /// are all refused — a volume root is the only thing the indexer ever completes.
    /// </summary>
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData(@"C:")]
    [InlineData(@"C:\WINDOWS")]
    [InlineData(@"c:\")]
    [InlineData(@"C:/")]
    [InlineData(@"\\SERVER\SHARE")]
    [InlineData(@"\\.\C:")]
    [InlineData(@"\\?\C:\")]
    [InlineData(@"..\")]
    [InlineData(@"\")]
    [InlineData(@"C:\*")]
    public void RejectsAnythingThatIsNotAVolumeRoot(string? root)
    {
        Assert.False(IndexProtocol.IsAcceptableRootKey(root));
    }

    [Fact]
    public void RejectsACompleteCarryingAControlCharacter()
    {
        Rejects("IDX\tComplete\tC:\\\u0000");
    }

    [Theory]
    [InlineData("Indexing C:…")]
    [InlineData("The database schema is newer than this helper.")]
    public void AcceptsAPlainStatusLine(string status)
    {
        Assert.True(IndexProtocol.IsAcceptableStatus(status));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("two\nlines")]
    [InlineData("a\u0000b")]
    public void RejectsAStatusThatIsEmptyOrCarriesControlCharacters(string? status)
    {
        Assert.False(IndexProtocol.IsAcceptableStatus(status));
    }

    [Fact]
    public void RejectsAnOverlongStatus()
    {
        Assert.False(IndexProtocol.IsAcceptableStatus(new string('a', IndexProtocol.MaxStatusLength + 1)));
        Assert.True(IndexProtocol.IsAcceptableStatus(new string('a', IndexProtocol.MaxStatusLength)));
    }

    [Theory]
    [InlineData("IDX\tHello\t0")]
    [InlineData("IDX\tHello\t-1")]
    [InlineData("IDX\tHello\tone")]
    [InlineData("IDX\tHello")]
    public void RejectsAHelloWithoutAPositiveVersion(string line)
    {
        Rejects(line);
    }

    [Fact]
    public void ReadsTheVersionOffAHello()
    {
        Assert.Equal(1, IndexProtocol.VersionOf(Parse("IDX\tHello\t1")));
        Assert.Equal(7, IndexProtocol.VersionOf(Parse("IDX\tHello\t7")));
        Assert.Null(IndexProtocol.VersionOf(Parse("IDX\tStart")));
    }

    /// <summary>
    /// Verbs that say everything by arriving must not smuggle a payload — that is what keeps the
    /// elevated surface free of anything an attacker chooses.
    /// </summary>
    [Theory]
    [InlineData("IDX\tStart\tC:\\WINDOWS")]
    [InlineData("IDX\tShutdown\tnow")]
    [InlineData("IDX\tPing\tanything")]
    [InlineData("IDX\tReady\tC:\\")]
    public void RejectsAnArgumentOnAVerbThatTakesNone(string line)
    {
        Rejects(line);
    }

    [Fact]
    public void TheElevatedSurfaceIsFourVerbsAndNoneOfThemNamesAPath()
    {
        IndexVerb[] acceptedByTheHelper = [IndexVerb.Hello, IndexVerb.Start, IndexVerb.Shutdown, IndexVerb.Ping];

        foreach (var verb in acceptedByTheHelper)
        {
            Assert.False(IndexProtocol.IsAcceptableArgument(verb, @"C:\Windows\System32"));
            Assert.False(IndexProtocol.IsAcceptableArgument(verb, @"\\server\share"));
        }
    }
}
