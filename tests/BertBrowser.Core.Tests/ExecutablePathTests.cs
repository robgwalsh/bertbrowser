using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The search order is the point. A launch no longer throws when a program is missing, so a null
/// return here is the only thing standing between "Windows Terminal isn't installed" and a menu
/// item that silently does nothing — and an absolute path is what stops an elevated process
/// resolving a bare name through a search path someone else can write to.
/// </summary>
public class ExecutablePathTests
{
    private const string PathExt = ".COM;.EXE;.BAT;.CMD";

    /// <summary>A filesystem that contains exactly what it is told to, compared the way Windows
    /// compares paths.</summary>
    private static Func<string, bool> Existing(params string[] paths)
    {
        var set = new HashSet<string>(paths, StringComparer.OrdinalIgnoreCase);
        return set.Contains;
    }

    /// <summary>Compared case-insensitively because Windows paths are: a name resolved by
    /// appending a <c>PATHEXT</c> entry carries that entry's casing (<c>.EXE</c>), and pinning the
    /// case would assert something neither Windows nor this code promises.</summary>
    private static void AssertPath(string expected, string? actual) =>
        Assert.Equal(expected, actual, StringComparer.OrdinalIgnoreCase);

    [Fact]
    public void AFullPathThatExistsIsTakenAsIs() =>
        AssertPath(
            @"C:\Program Files\Microsoft VS Code\Code.exe",
            ExecutablePath.Resolve(@"C:\Program Files\Microsoft VS Code\Code.exe", @"C:\Windows", PathExt,
                Existing(@"C:\Program Files\Microsoft VS Code\Code.exe")));

    [Fact]
    public void AFullPathThatIsGoneIsNull_AndIsNotLookedForOnThePath()
    {
        // The name is on the PATH, but the caller asked for a specific file. Falling back would
        // start a different program than the one named.
        var resolved = ExecutablePath.Resolve(
            @"C:\gone\tool.exe", @"C:\Windows", PathExt, Existing(@"C:\Windows\tool.exe"));

        Assert.Null(resolved);
    }

    [Fact]
    public void ARelativePathWithASeparatorIsNotSearchedOnThePath() =>
        Assert.Null(ExecutablePath.Resolve(
            @"sub\tool.exe", @"C:\Windows", PathExt, Existing(@"C:\Windows\tool.exe")));

    [Theory]
    // Each of these resolves against the current directory, which for an elevated process is
    // wherever it was started from. Whatever is there must never get to decide what runs.
    [InlineData(@"sub\tool.exe")]
    [InlineData(@".\tool.exe")]
    [InlineData(@"..\tool.exe")]
    [InlineData("/tool.exe")]
    [InlineData(@"\tool.exe")]
    public void APathThatIsNotFullyQualifiedIsNeverProbed(string program)
    {
        var probed = new List<string>();

        var resolved = ExecutablePath.Resolve(program, @"C:\Windows", PathExt, path =>
        {
            probed.Add(path);
            return true; // a filesystem that says yes to everything
        });

        Assert.Null(resolved);
        Assert.Empty(probed);
    }

    [Fact]
    public void ABareNameIsFoundOnThePath() =>
        AssertPath(
            @"C:\Windows\System32\wt.exe",
            ExecutablePath.Resolve("wt.exe", @"C:\Windows;C:\Windows\System32", PathExt,
                Existing(@"C:\Windows\System32\wt.exe")));

    [Fact]
    public void ThePathIsSearchedInOrder() =>
        AssertPath(
            @"C:\first\tool.exe",
            ExecutablePath.Resolve("tool.exe", @"C:\first;C:\second", PathExt,
                Existing(@"C:\first\tool.exe", @"C:\second\tool.exe")));

    [Fact]
    public void ANameWithNoExtensionPicksUpOneFromPathExt() =>
        AssertPath(
            @"C:\tools\code.cmd",
            ExecutablePath.Resolve("code", @"C:\tools", PathExt, Existing(@"C:\tools\code.cmd")));

    [Fact]
    public void PathExtIsTriedInItsOwnOrder() =>
        AssertPath(
            @"C:\tools\code.exe",
            ExecutablePath.Resolve("code", @"C:\tools", PathExt,
                Existing(@"C:\tools\code.exe", @"C:\tools\code.cmd")));

    [Fact]
    public void ExtensionsAreNotAppendedToANameThatAlreadyHasOne()
    {
        // Otherwise "wt.exe" could be answered by "wt.exe.bat" sitting earlier on the PATH.
        var resolved = ExecutablePath.Resolve(
            "wt.exe", @"C:\tools", PathExt, Existing(@"C:\tools\wt.exe.bat"));

        Assert.Null(resolved);
    }

    [Fact]
    public void AProgramThatIsNotThereIsNull() =>
        Assert.Null(ExecutablePath.Resolve("wt.exe", @"C:\Windows;C:\tools", PathExt, Existing()));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("\"\"")]
    public void NothingResolvesToNothing(string? program) =>
        Assert.Null(ExecutablePath.Resolve(program, @"C:\Windows", PathExt, Existing(@"C:\Windows\tool.exe")));

    [Fact]
    public void QuotedPathEntriesAreUnderstood() =>
        AssertPath(
            @"C:\Program Files\tools\tool.exe",
            ExecutablePath.Resolve("tool.exe", "\"C:\\Program Files\\tools\"", PathExt,
                Existing(@"C:\Program Files\tools\tool.exe")));

    [Fact]
    public void AQuotedProgramIsUnderstood() =>
        AssertPath(
            @"C:\tools\tool.exe",
            ExecutablePath.Resolve("\"C:\\tools\\tool.exe\"", @"C:\Windows", PathExt,
                Existing(@"C:\tools\tool.exe")));

    [Fact]
    public void EmptyAndPaddedPathEntriesAreSkippedRatherThanEndingTheSearch() =>
        AssertPath(
            @"C:\tools\tool.exe",
            ExecutablePath.Resolve("tool.exe", @";; C:\nope ;;C:\tools;", PathExt,
                Existing(@"C:\tools\tool.exe")));

    [Fact]
    public void AnUnusablePathEntryDoesNotStopTheSearch() =>
        AssertPath(
            @"C:\tools\tool.exe",
            ExecutablePath.Resolve("tool.exe", "C:\\bad|entry;C:\\tools", PathExt,
                Existing(@"C:\tools\tool.exe")));

    [Fact]
    public void AnEmptyPathFindsNothingRatherThanThrowing() =>
        Assert.Null(ExecutablePath.Resolve("tool.exe", "", PathExt, Existing(@"C:\tools\tool.exe")));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void ABlankPathExtFallsBackToTheWindowsDefault(string? pathExt) =>
        AssertPath(
            @"C:\tools\code.cmd",
            ExecutablePath.Resolve("code", @"C:\tools", pathExt, Existing(@"C:\tools\code.cmd")));

    [Fact]
    public void PathExtEntriesMayOmitTheirLeadingDot() =>
        AssertPath(
            @"C:\tools\code.cmd",
            ExecutablePath.Resolve("code", @"C:\tools", "COM;EXE;BAT;CMD", Existing(@"C:\tools\code.cmd")));

    [Fact]
    public void AProbeThatThrowsCostsThatCandidateAndNotTheSearch()
    {
        var resolved = ExecutablePath.Resolve("tool.exe", @"C:\bad;C:\tools", PathExt,
            path => path.StartsWith(@"C:\bad", StringComparison.OrdinalIgnoreCase)
                ? throw new IOException("device not ready")
                : string.Equals(path, @"C:\tools\tool.exe", StringComparison.OrdinalIgnoreCase));

        Assert.Equal(@"C:\tools\tool.exe", resolved);
    }

    [Fact]
    public void MetaTheFakeFilesystemReallyCanFail() =>
        // If Existing() answered true for everything the search-order tests above would pass no
        // matter what Resolve did.
        Assert.False(Existing(@"C:\tools\tool.exe")(@"C:\tools\other.exe"));
}
