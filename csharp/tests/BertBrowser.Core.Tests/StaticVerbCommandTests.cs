using BertBrowser.Core.Services.ShellMenu;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>What a static registry verb runs, for the folder it was clicked in.</summary>
public sealed class StaticVerbCommandTests
{
    private const string GitBash = @"C:\Program Files\Git\git-bash.exe";
    private const string Folder = @"C:\Source\app";

    private static Func<string, bool> Real(params string[] paths) =>
        path => paths.Contains(path, StringComparer.OrdinalIgnoreCase);

    /// <summary>Git for Windows' background verb, verbatim — the trailing dot after <c>%v</c> is
    /// theirs and has to survive.</summary>
    [Fact]
    public void GitBashHereOnTheBackground()
    {
        var command = StaticVerbCommand.Resolve(
            $"\"{GitBash}\" \"--cd=%v.\"", Folder, Folder, Real(GitBash));

        Assert.NotNull(command);
        Assert.Equal(GitBash, command.Executable);
        Assert.Equal($"\"--cd={Folder}.\"", command.Arguments);
    }

    [Fact]
    public void TheWorkingDirectoryPlaceholderIsTheFolderNotTheItem()
    {
        var command = StaticVerbCommand.Resolve(
            $"\"{GitBash}\" \"--cd=%W\" \"%1\"", @"C:\Source\app\readme.md", Folder, Real(GitBash));

        Assert.Equal($"\"--cd={Folder}\" \"C:\\Source\\app\\readme.md\"", command!.Arguments);
    }

    [Fact]
    public void AVerbWithNoPlaceholderStillRuns()
    {
        var command = StaticVerbCommand.Resolve($"\"{GitBash}\" --login", Folder, Folder, Real(GitBash));

        Assert.Equal("--login", command!.Arguments);
    }

    [Fact]
    public void AProgramThatIsNotThereIsNothingToRun() =>
        Assert.Null(StaticVerbCommand.Resolve($"\"{GitBash}\" \"%1\"", Folder, Folder, Real()));

    [Fact]
    public void ABlankCommandIsNothingToRun() =>
        Assert.Null(StaticVerbCommand.Resolve("  ", Folder, Folder, Real(GitBash)));

    [Fact]
    public void ABackgroundVerbRunsInTheFolder() =>
        Assert.Equal(Folder, StaticVerbCommand.WorkingDirectoryFor(
            new ShellMenuTarget(Folder, true), ShellMenuContext.Background, Folder));

    [Fact]
    public void AFileVerbRunsInTheFilesOwnFolder() =>
        Assert.Equal(@"C:\Source\app\src", StaticVerbCommand.WorkingDirectoryFor(
            new ShellMenuTarget(@"C:\Source\app\src\a.cs", false), ShellMenuContext.Items, Folder));

    [Fact]
    public void AFolderVerbRunsInThatFolder() =>
        Assert.Equal(@"C:\Source\app\src", StaticVerbCommand.WorkingDirectoryFor(
            new ShellMenuTarget(@"C:\Source\app\src", true), ShellMenuContext.Items, Folder));
}
