using BertBrowser.Core.Cli;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// <see cref="NavigationRequest.IsAcceptablePath"/> is the one rule standing between an elevated
/// process and a path handed to it over a pipe. Mutate it to accept and the refusal theories below
/// go red — the same guarantee <c>ThemeIdTests</c> gives the theme loader.
/// </summary>
public sealed class NavigationRequestTests
{
    // --- what is allowed ---

    [Theory]
    [InlineData(@"C:\")]
    [InlineData(@"C:\Dir")]
    [InlineData(@"C:\Dir\file.txt")]
    [InlineData(@"C:\Program Files\Some App")]
    [InlineData(@"\\server\share")]
    [InlineData(@"\\server\share\folder")]
    [InlineData(@"D:\folder with spaces\and-dashes")]
    public void AnAbsoluteLocalOrNetworkPath_IsAccepted(string path)
    {
        Assert.True(NavigationRequest.IsAcceptablePath(path));
    }

    // --- what is refused ---

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void NothingAtAll_IsRefused(string? path)
    {
        Assert.False(NavigationRequest.IsAcceptablePath(path));
    }

    /// <summary>A relative path resolves against whatever directory this process happens to be in,
    /// which is not something the sender can know.</summary>
    [Theory]
    [InlineData("Dir")]
    [InlineData(@"..\..\Windows")]
    [InlineData(@".\Dir")]
    [InlineData(@"\Dir")] // rooted, but on "the current drive" — the same ambiguity
    [InlineData("C:Dir")] // drive-relative
    public void ARelativePath_IsRefused(string path)
    {
        Assert.False(NavigationRequest.IsAcceptablePath(path));
    }

    /// <summary>These do not name files at all, and this process is elevated.</summary>
    [Theory]
    [InlineData(@"\\.\PhysicalDrive0")]
    [InlineData(@"\\.\C:")]
    [InlineData(@"\\?\C:\Dir")]
    public void AWin32DevicePath_IsRefused(string path)
    {
        Assert.False(NavigationRequest.IsAcceptablePath(path));
    }

    /// <summary>A newline would let one message forge a second; a NUL truncates on the way into
    /// Win32; a tab would break the format apart from the inside.</summary>
    [Theory]
    [InlineData("C:\\Dir\nOPEN\tDefault")]
    [InlineData("C:\\Dir\r\n")]
    [InlineData("C:\\Dir\0extra")]
    [InlineData("C:\\Dir\tmore")]
    public void AControlCharacter_IsRefused(string path)
    {
        Assert.False(NavigationRequest.IsAcceptablePath(path));
    }

    [Theory]
    [InlineData(@"C:\Dir\*")]
    [InlineData(@"C:\Dir\?.txt")]
    [InlineData(@"C:\Dir\a|b")]
    [InlineData("C:\\Dir\\a\"b")]
    public void AWildcardOrOtherIllegalCharacter_IsRefused(string path)
    {
        Assert.False(NavigationRequest.IsAcceptablePath(path));
    }

    [Fact]
    public void AnImplausiblyLongPath_IsRefused()
    {
        Assert.False(NavigationRequest.IsAcceptablePath(@"C:\" + new string('a', 5000)));
    }

    // --- the wire format ---

    [Fact]
    public void ARequestSurvivesTheRoundTrip()
    {
        var original = new CommandLineRequest(
            [new OpenTarget(@"C:\Dir", false), new OpenTarget(@"C:\Other\file.txt", true)],
            OpenIn.NewTab,
            []);

        Assert.True(NavigationRequest.TryParse(NavigationRequest.Format(original), out var parsed));

        Assert.Equal(OpenIn.NewTab, parsed.Mode);
        Assert.Equal(2, parsed.Targets.Count);
        Assert.Equal(@"C:\Dir", parsed.Targets[0].Path);
        Assert.False(parsed.Targets[0].Select);
        Assert.Equal(@"C:\Other\file.txt", parsed.Targets[1].Path);
        Assert.True(parsed.Targets[1].Select);
    }

    [Fact]
    public void APathWithSpaces_SurvivesTheRoundTrip()
    {
        var original = new CommandLineRequest(
            [new OpenTarget(@"C:\Program Files\Some App", false)], OpenIn.Default, []);

        Assert.True(NavigationRequest.TryParse(NavigationRequest.Format(original), out var parsed));
        Assert.Equal(@"C:\Program Files\Some App", Assert.Single(parsed.Targets).Path);
    }

    [Fact]
    public void ARequestWithNoTargets_StillRoundTrips()
    {
        Assert.True(NavigationRequest.TryParse(
            NavigationRequest.Format(CommandLineRequest.Empty), out var parsed));
        Assert.Empty(parsed.Targets);
    }

    /// <summary>The formatter is the second line of defence, not just the parser: an unacceptable
    /// path must not even go on the wire.</summary>
    [Fact]
    public void FormattingDropsAPathThatWouldNotBeAccepted()
    {
        var request = new CommandLineRequest(
            [new OpenTarget(@"..\..\Windows", false), new OpenTarget(@"C:\Dir", false)],
            OpenIn.Default,
            []);

        Assert.True(NavigationRequest.TryParse(NavigationRequest.Format(request), out var parsed));
        Assert.Equal(@"C:\Dir", Assert.Single(parsed.Targets).Path);
    }

    [Theory]
    [InlineData("")]
    [InlineData("garbage")]
    [InlineData("OPEN")]
    [InlineData("NOTOPEN\tDefault")]
    [InlineData("OPEN\tNotAMode")]
    public void AMalformedLine_IsRejected(string line)
    {
        Assert.False(NavigationRequest.TryParse(line, out _));
    }

    [Fact]
    public void AnOversizedLine_IsRejected()
    {
        Assert.False(NavigationRequest.TryParse(
            "OPEN\tDefault\t-" + new string('a', NavigationRequest.MaxLineLength), out _));
    }

    /// <summary>A half-corrupt request opens the parts that were sound rather than nothing at all.</summary>
    [Fact]
    public void ABadFieldIsDropped_WithoutCostingTheGoodOnes()
    {
        Assert.True(NavigationRequest.TryParse("OPEN\tDefault\tXnonsense\t-C:\\Dir", out var parsed));
        Assert.Equal(@"C:\Dir", Assert.Single(parsed.Targets).Path);
    }

    /// <summary>The parser re-checks rather than trusting whoever wrote the line: the sender is
    /// another process, not necessarily this code.</summary>
    [Fact]
    public void AnUnacceptablePathOnTheWire_IsDroppedByTheParserToo()
    {
        Assert.True(NavigationRequest.TryParse("OPEN\tDefault\t-..\\..\\Windows", out var parsed));
        Assert.Empty(parsed.Targets);
    }
}
