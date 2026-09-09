using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The security file. A checksum file is written by whoever made the download, and every name in
/// it is about to become a path this app opens — so each of these is a line somebody could put in
/// one, and the answer to all of them is "no", returned rather than thrown.
/// </summary>
public sealed class ChecksumPathTests
{
    private const string Folder = @"C:\Downloads\release";

    private static string? Resolve(string name) => ChecksumPath.Resolve(Folder, name);

    // --- what must be refused ---

    [Theory]
    [InlineData(@"..\..\Windows\System32\config\SAM")]
    [InlineData("../../etc/passwd")]
    [InlineData(@"sub\..\..\outside.txt")]
    [InlineData("..")]
    [InlineData(@"..\sibling.txt")]
    public void AnEscapingPathIsRefused(string name) => Assert.Null(Resolve(name));

    [Theory]
    [InlineData(@"C:\Windows\System32\drivers\etc\hosts")]
    [InlineData(@"\\server\share\payload.dll")]
    [InlineData(@"\Windows\notepad.exe")]
    [InlineData(@"C:relative.txt")]
    [InlineData(@"\\?\C:\Windows\notepad.exe")]
    [InlineData(@"\\.\PhysicalDrive0")]
    public void ARootedOrDevicePathIsRefused(string name) => Assert.Null(Resolve(name));

    [Theory]
    [InlineData("bad<name.txt")]
    [InlineData("pipe|name.txt")]
    [InlineData("star*.txt")]
    [InlineData("quote\"name.txt")]
    public void AnInvalidNameIsRefused(string name) => Assert.Null(Resolve(name));

    [Fact]
    public void AnEmptyNameIsRefused()
    {
        Assert.Null(Resolve(""));
        Assert.Null(Resolve("   "));
    }

    [Fact]
    public void ACurrentDirectorySegmentIsRefused() => Assert.Null(Resolve(@".\file.txt"));

    // --- what must be allowed ---

    [Fact]
    public void APlainNameResolvesUnderTheFolder() =>
        Assert.Equal(@"C:\Downloads\release\setup.exe", Resolve("setup.exe"));

    /// <summary>
    /// sha256sum files are written on Linux at least as often as on Windows, so a listed subpath
    /// arrives with forward slashes and has to work.
    /// </summary>
    [Fact]
    public void AForwardSlashSubPathResolves() =>
        Assert.Equal(@"C:\Downloads\release\bin\tool.exe", Resolve("bin/tool.exe"));

    [Fact]
    public void ABackslashSubPathResolves() =>
        Assert.Equal(@"C:\Downloads\release\bin\tool.exe", Resolve(@"bin\tool.exe"));

    [Fact]
    public void ANameWithSpacesResolves() =>
        Assert.Equal(@"C:\Downloads\release\my file.zip", Resolve("my file.zip"));

    // --- writing them back out ---

    [Fact]
    public void RelativiseUsesForwardSlashes() =>
        Assert.Equal("bin/tool.exe", ChecksumPath.Relativise(Folder, @"C:\Downloads\release\bin\tool.exe"));

    [Fact]
    public void RelativiseRefusesSomethingOutsideTheFolder() =>
        Assert.Null(ChecksumPath.Relativise(Folder, @"C:\Windows\notepad.exe"));

    /// <summary>Every refusal above is a null. Nothing here may throw — one bad line is one row of
    /// a report, not the end of the run.</summary>
    [Fact]
    public void NothingThrows()
    {
        foreach (var name in new[] { "", "..", "\0", new string('x', 5000), @"C:\", "|", "con:" })
        {
            _ = ChecksumPath.Resolve(Folder, name);
        }
    }
}
