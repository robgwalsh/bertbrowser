using BertBrowser.Core.Services.ShellIntegration;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Whether "Run as administrator" can mean anything for a given file.
/// </summary>
/// <remarks>
/// The bug this exists to prevent: the menu item was enabled for every file, so choosing it on a
/// text file produced <c>ERROR_NO_ASSOCIATION</c> and a status-bar line reading "No application is
/// associated with the specified file for this operation" — about a file that opens fine on a
/// double-click. From the user's side it looked like nothing happened at all.
/// </remarks>
public class RunAsVerbRulesTests
{
    private static readonly Func<string, bool?> RegistrySaysNo = _ => false;
    private static readonly Func<string, bool?> RegistrySaysYes = _ => true;
    private static readonly Func<string, bool?> RegistryCannotTell = _ => null;

    // --- the extension to ask about ---

    [Theory]
    [InlineData(@"C:\tools\setup.exe", ".exe")]
    [InlineData(@"C:\tools\SETUP.EXE", ".exe")]
    [InlineData(@"C:\notes\report.md", ".md")]
    [InlineData(@"C:\a\archive.tar.gz", ".gz")]
    public void TheExtensionIsWhatTheShellWouldAskAbout(string path, string expected) =>
        Assert.Equal(expected, RunAsVerbRules.ExtensionOf(path));

    [Theory]
    [InlineData(@"C:\src\.gitignore")]  // a dotfile is a name, not a type
    [InlineData(@"C:\bin\Makefile")]    // no extension at all
    [InlineData(@"C:\a\trailing.")]     // nothing after the dot
    [InlineData("")]
    public void SomethingWithNoTypeHasNothingToAskAbout(string path) =>
        Assert.Equal("", RunAsVerbRules.ExtensionOf(path));

    // --- the decision ---

    [Fact]
    public void AProgramCanBeRunAsAdministrator() =>
        Assert.True(RunAsVerbRules.CanRunElevated(
            @"C:\tools\setup.exe", isDirectory: false, insideArchive: false, RegistrySaysYes));

    [Fact]
    public void ADocumentCannot() =>
        // The reported bug, in one line.
        Assert.False(RunAsVerbRules.CanRunElevated(
            @"C:\notes\notes.txt", isDirectory: false, insideArchive: false, RegistrySaysNo));

    [Fact]
    public void AFolderCannot() =>
        Assert.False(RunAsVerbRules.CanRunElevated(
            @"C:\tools", isDirectory: true, insideArchive: false, RegistrySaysYes));

    [Fact]
    public void SomethingInsideAnArchiveCannot() =>
        // It has no path another process could be pointed at; it has to be extracted first.
        Assert.False(RunAsVerbRules.CanRunElevated(
            @"C:\a\bundle.zip\setup.exe", isDirectory: false, insideArchive: true, RegistrySaysYes));

    [Fact]
    public void SomethingWithNoExtensionCannot() =>
        Assert.False(RunAsVerbRules.CanRunElevated(
            @"C:\bin\Makefile", isDirectory: false, insideArchive: false, RegistrySaysYes));

    [Fact]
    public void AFileTypeSomethingElseRegisteredAVerbForCan() =>
        // The reason the registry is asked rather than an extension list consulted: greying the item
        // out on something that would have worked is the worse failure of the two.
        Assert.True(RunAsVerbRules.CanRunElevated(
            @"C:\vendor\thing.customapp", isDirectory: false, insideArchive: false, RegistrySaysYes));

    [Fact]
    public void AShortcutCanEvenThoughTheRegistrySaysOtherwise()
    {
        // The one place the registry is not the authority. A .lnk carries no verbs of its own — the
        // shell resolves it and applies the target's — so lnkfile has no runas key, and trusting the
        // probe would grey the item out on a shortcut to a program Windows will happily elevate.
        // Measured: runas on a shortcut to notepad.exe starts an elevated Notepad.
        Assert.True(RunAsVerbRules.CanRunElevated(
            @"C:\Desktop\Notepad.lnk", isDirectory: false, insideArchive: false, RegistrySaysNo));
    }

    [Fact]
    public void AShortcutIsNotEvenAskedAbout()
    {
        var asked = false;
        RunAsVerbRules.CanRunElevated(
            @"C:\Desktop\Notepad.lnk", isDirectory: false, insideArchive: false,
            _ => { asked = true; return false; });

        Assert.False(asked, "a shortcut is settled by the shell, not by the registry.");
    }

    // --- when the registry cannot be read ---

    [Theory]
    [InlineData(".exe")]
    [InlineData(".bat")]
    [InlineData(".cmd")]
    [InlineData(".msc")]
    public void AnUnreadableRegistryStillLetsAProgramRun(string extension) =>
        // Not a refusal: being unable to read the registry must not take the feature away from an
        // .exe, and everything on the fallback list is a program by any reading.
        Assert.True(RunAsVerbRules.CanRunElevated(
            $@"C:\tools\thing{extension}", isDirectory: false, insideArchive: false, RegistryCannotTell));

    [Fact]
    public void AnUnreadableRegistryDoesNotGuessAboutADocument() =>
        Assert.False(RunAsVerbRules.CanRunElevated(
            @"C:\notes\notes.txt", isDirectory: false, insideArchive: false, RegistryCannotTell));

    [Fact]
    public void TheExtensionIsAskedAboutInTheFormTheRegistryUses()
    {
        // Lower-cased and carrying its dot, or the lookup misses on a file named in capitals.
        string? asked = null;
        RunAsVerbRules.CanRunElevated(
            @"C:\tools\SETUP.EXE", isDirectory: false, insideArchive: false,
            extension => { asked = extension; return true; });

        Assert.Equal(".exe", asked);
    }

    // --- the second way: elevating the file's handler ---

    [Fact]
    public void ATypeWithNoVerbButAHandlerElevatesTheHandler()
    {
        // The .sln case. No progid under it carries a runas verb, but it does have an open command,
        // and starting VSLauncher elevated with the solution is what the user meant.
        var exe = @"C:\VS\VSLauncher.exe";

        var open = RunAsVerbRules.Decide(
            @"C:\Source\app.sln", isDirectory: false, insideArchive: false,
            hasVerb: _ => false,
            openCommand: _ => $"\"{exe}\" \"%1\"",
            exists: p => p == exe);

        Assert.Equal(ElevatedOpenKind.Handler, open.Kind);
        Assert.Equal(exe, open.Executable);
        Assert.Equal(@"""C:\Source\app.sln""", open.Arguments);
    }

    [Fact]
    public void ARegisteredVerbIsPreferredOverTheHandler()
    {
        // An .exe has a real runas verb; handing it to the shell is what Windows means by running it
        // as administrator, and second-guessing that with its own open command would be wrong.
        var open = RunAsVerbRules.Decide(
            @"C:\tools\setup.exe", isDirectory: false, insideArchive: false,
            hasVerb: _ => true,
            openCommand: _ => @"""C:\other\thing.exe"" ""%1""",
            exists: _ => true);

        Assert.Equal(ElevatedOpenKind.Verb, open.Kind);
    }

    [Fact]
    public void ATypeWithNeitherIsStillRefused()
    {
        var open = RunAsVerbRules.Decide(
            @"C:\notes\notes.txt", isDirectory: false, insideArchive: false,
            hasVerb: _ => false,
            openCommand: _ => null,
            exists: _ => true);

        Assert.Equal(ElevatedOpenKind.None, open.Kind);
    }

    [Fact]
    public void AHandlerThatCannotBeResolvedIsNotGuessedAt() =>
        // Greying the item out is the right failure. Starting an approximation with a token is not.
        Assert.Equal(
            ElevatedOpenKind.None,
            RunAsVerbRules.Decide(
                @"C:\Source\app.sln", isDirectory: false, insideArchive: false,
                hasVerb: _ => false,
                openCommand: _ => @"""C:\gone\missing.exe"" ""%1""",
                exists: _ => false).Kind);

    [Fact]
    public void AFolderHasNoHandlerToElevateEither() =>
        Assert.Equal(
            ElevatedOpenKind.None,
            RunAsVerbRules.Decide(
                @"C:\Source", isDirectory: true, insideArchive: false,
                hasVerb: _ => true,
                openCommand: _ => @"""C:\VS\VSLauncher.exe"" ""%1""",
                exists: _ => true).Kind);

    // --- what it says when it was asked for anyway ---

    [Fact]
    public void TheMessageNamesTheFileAndSaysWhy()
    {
        var message = RunAsVerbRules.CannotRunMessage("notes.txt");

        Assert.Contains("notes.txt", message, StringComparison.Ordinal);
        Assert.DoesNotContain("associated", message, StringComparison.OrdinalIgnoreCase);
    }
}
