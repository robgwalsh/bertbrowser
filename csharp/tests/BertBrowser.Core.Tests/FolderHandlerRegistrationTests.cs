using BertBrowser.Core.Services.ShellIntegration;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The rule that decides whether the Windows shell opens folders in BertBrowser. Nothing here
/// touches the registry — the App half reads and writes raw values, and every decision about what
/// they mean lives in Core so it can be tested in a project that cannot open a key.
/// </summary>
public sealed class FolderHandlerRegistrationTests
{
    private const string Exe = @"C:\Users\Rob\AppData\Local\BertBrowser\current\BertBrowser.exe";

    /// <summary>Another file manager holding the verb — a real one, since the point of the rule is
    /// that BertBrowser must not quietly take a registration it did not make.</summary>
    private const string ForeignProgram = @"C:\Program Files\Files\Files.exe";
    private const string ForeignCommand = @"""C:\Program Files\Files\Files.exe"" ""%1""";

    /// <summary>A complete registration: both commands, and both shell keys naming the verb as
    /// their default action — without which the shell never invokes it.</summary>
    private static FolderHandlerReading Registered(string exe = Exe) =>
        new("open", Command(exe), "open", Command(exe));

    private static string Command(string exe) => FolderHandlerRegistration.CommandFor(exe);

    // --- what gets written ---

    [Fact]
    public void TheCommandQuotesBothTheProgramAndTheArgument() =>
        Assert.Equal("\"" + Exe + "\" --new-tab \"%1\"", Command(Exe));

    /// <summary>
    /// Opening from the shell is a new tab, never a retarget of whichever tab happened to be
    /// active — double-clicking a folder in Explorer shows you that folder, it does not take away
    /// the one you were using. The flag lives on the registration so that
    /// <c>bertbrowser &lt;path&gt;</c> typed at a prompt keeps its own documented behaviour.
    /// </summary>
    [Fact]
    public void TheShellIsToldToOpenANewTab()
    {
        var request = BertBrowser.Core.Cli.CommandLine.Parse(
            [FolderHandlerRegistration.NewTabFlag, @"C:\Some\Folder"]);

        Assert.Equal(BertBrowser.Core.Cli.OpenIn.NewTab, request.Mode);
        Assert.Equal(@"C:\Some\Folder", Assert.Single(request.Targets).Path);
        Assert.Empty(request.Errors);
    }

    /// <summary>The flag must not be mistaken for a path, nor the path for a flag, once the shell
    /// has substituted a real folder into <c>%1</c>.</summary>
    [Fact]
    public void TheRegisteredCommandParsesBackIntoWhatItMeant()
    {
        var command = Command(Exe);
        var argv = command[(command.IndexOf("\" ", StringComparison.Ordinal) + 2)..]
            .Replace("\"%1\"", @"C:\Users\Rob\Documents");

        var request = BertBrowser.Core.Cli.CommandLine.Parse(argv.Split(' '));

        Assert.Equal(BertBrowser.Core.Cli.OpenIn.NewTab, request.Mode);
        Assert.Equal(@"C:\Users\Rob\Documents", Assert.Single(request.Targets).Path);
    }

    /// <summary>
    /// <b>The regression test for the worst bug this feature has had.</b> Naming the default verb
    /// is what makes the shell invoke it at all — but naming one that is not yet there sends a
    /// folder double-click to whatever third-party verb enumerates first, which on a real machine
    /// meant NordVPN. So the value lives apart from the verb it names, and
    /// <c>FolderHandlerRegistry.TryRegister</c> writes it only after reading the command back.
    /// </summary>
    [Fact]
    public void TheDefaultVerbIsNotWrittenAlongsideTheVerbItNames()
    {
        Assert.DoesNotContain(
            FolderHandlerRegistration.ValuesFor(Exe),
            v => (v.KeyPath == FolderHandlerRegistration.DirectoryShellKey
                  || v.KeyPath == FolderHandlerRegistration.DriveShellKey)
                 && v.ValueName == "");

        Assert.Equal(2, FolderHandlerRegistration.GuardValues.Count);
        Assert.All(FolderHandlerRegistration.GuardValues, v =>
        {
            Assert.Equal("", v.ValueName);
            Assert.Equal(FolderHandlerRegistration.OpenVerb, v.Data);
        });
    }

    /// <summary>The keys whose command must read back before the default verb may name them — the
    /// two that <see cref="FolderHandlerRegistration.GuardValues"/> point at.</summary>
    [Fact]
    public void TheVerifiedCommandKeysAreTheOnesTheDefaultVerbNames() =>
        Assert.Equal(
            [FolderHandlerRegistration.DirectoryCommandKey, FolderHandlerRegistration.DriveCommandKey],
            FolderHandlerRegistration.CommandKeys);

    /// <summary>
    /// The command is written before the key that carries the verb's decoration. Creating the
    /// <c>open</c> key registers a verb; the command is what makes it runnable, so an interrupted
    /// write must not be able to leave the first without the second.
    /// </summary>
    [Fact]
    public void TheCommandIsWrittenBeforeTheVerbKey()
    {
        var values = FolderHandlerRegistration.ValuesFor(Exe);

        Assert.True(
            values.ToList().FindIndex(v => v.KeyPath == FolderHandlerRegistration.DirectoryCommandKey) <
            values.ToList().FindIndex(v => v.KeyPath == FolderHandlerRegistration.DirectoryOpenKey));
    }

    /// <summary>
    /// Without this the whole feature is inert. <c>Directory</c> and <c>Drive</c> inherit their
    /// <c>open</c> verb from <c>Folder</c>, which carries <c>CLSID_ExecuteFolder</c> in
    /// <c>DelegateExecute</c> — and a <c>DelegateExecute</c> that resolves makes the shell ignore
    /// the command line entirely, sending every folder to Explorer while these keys look correct.
    /// The empty shadow is what cuts that inheritance.
    /// </summary>
    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void EachCommandBlanksTheInheritedDelegateExecute(bool directory)
    {
        var key = directory
            ? FolderHandlerRegistration.DirectoryCommandKey
            : FolderHandlerRegistration.DriveCommandKey;

        Assert.Contains(
            FolderHandlerRegistration.ValuesFor(Exe),
            v => v.KeyPath == key && v.ValueName == "DelegateExecute" && v.Data == "");
    }

    [Fact]
    public void BothClassesGetACommand()
    {
        var values = FolderHandlerRegistration.ValuesFor(Exe);

        Assert.Contains(values, v => v.KeyPath == FolderHandlerRegistration.DirectoryCommandKey && v.Data == Command(Exe));
        Assert.Contains(values, v => v.KeyPath == FolderHandlerRegistration.DriveCommandKey && v.Data == Command(Exe));
    }

    /// <summary>The <c>Folder</c> class is what This PC, Control Panel, zip browsing and Explorer's
    /// own navigation come through. Taking it is out of scope, and writing it by accident is the
    /// one mistake here that would break parts of Windows BertBrowser cannot stand in for.</summary>
    [Fact]
    public void NothingIsWrittenUnderTheFolderClass() =>
        Assert.DoesNotContain(
            FolderHandlerRegistration.ValuesFor(Exe),
            v => v.KeyPath.Contains(@"Classes\Folder", StringComparison.OrdinalIgnoreCase));

    /// <summary><c>Directory\shell</c> holds other installers' verbs, so unregistering deletes our
    /// verb subtree and our one value, never the key they share.</summary>
    [Fact]
    public void RemovalTakesTheVerbSubtreeButNotTheSharedShellKey()
    {
        Assert.Equal(
            [FolderHandlerRegistration.DirectoryOpenKey, FolderHandlerRegistration.DriveOpenKey],
            FolderHandlerRegistration.KeysToRemove);

        Assert.DoesNotContain(FolderHandlerRegistration.KeysToRemove, k => k == FolderHandlerRegistration.DirectoryShellKey);
        Assert.Contains(
            FolderHandlerRegistration.ValuesToRemove,
            v => v.KeyPath == FolderHandlerRegistration.DirectoryShellKey && v.ValueName == "");
    }

    /// <summary>
    /// The default verb is removed only when it still holds what this app wrote. Those keys are
    /// shared, and blanking a verb another program has since pointed elsewhere would break their
    /// registration in the name of tidying up after ours. Found the hard way: an unregister that
    /// deleted whatever it found removed a pre-existing empty key on a real machine.
    /// </summary>
    [Fact]
    public void OnlyTheDefaultVerbThisAppWroteIsRemovable() =>
        Assert.All(
            FolderHandlerRegistration.ValuesToRemove,
            v => Assert.Equal(FolderHandlerRegistration.OpenVerb, v.ExpectedData));

    // --- what a reading means ---

    [Fact]
    public void NothingRegisteredReadsAsNotRegistered() =>
        Assert.Equal(
            FolderHandlerState.NotRegistered,
            FolderHandlerRules.Classify(FolderHandlerReading.None, Exe));

    [Fact]
    public void ACompleteRegistrationNamingThisExeIsCurrent() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisApp,
            FolderHandlerRules.Classify(Registered(), Exe));

    /// <summary>Paths are compared through <see cref="BertBrowser.Core.Paths.PathKey"/>, not by
    /// string equality — otherwise a registration differing only in casing reads as stale and the
    /// startup self-heal rewrites it on every single launch.</summary>
    [Fact]
    public void CasingAloneDoesNotMakeARegistrationStale() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisApp,
            FolderHandlerRules.Classify(Registered(Exe.ToUpperInvariant()), Exe));

    /// <summary>
    /// An older build's command line — right program, outdated arguments — still launches the app
    /// and no longer means what this version means by it. Comparing only the program would leave it
    /// in place forever, so it is stale and the startup repair rewrites it.
    /// </summary>
    [Fact]
    public void AnOutdatedArgumentListIsStale() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisAppStale,
            FolderHandlerRules.Classify(new("open", $"\"{Exe}\" \"%1\"", "open", $"\"{Exe}\" \"%1\""), Exe));

    [Theory]
    [InlineData(@"""C:\a b\app.exe"" --new-tab ""%1""", @"--new-tab ""%1""")]
    [InlineData(@"C:\dir\app.exe --new-tab ""%1""", @"--new-tab ""%1""")]
    [InlineData(@"""C:\a b\app.exe""", "")]
    [InlineData(@"C:\dir\app.exe", "")]
    [InlineData(null, "")]
    public void TheArgumentsAreReadOffTheCommand(string? command, string expected) =>
        Assert.Equal(expected, FolderHandlerRules.ArgumentsIn(command));

    [Fact]
    public void AnOldInstallPathIsStale() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisAppStale,
            FolderHandlerRules.Classify(Registered(@"D:\Old\BertBrowser.exe"), Exe));

    [Fact]
    public void OnlyHalfTheRegistrationIsStale() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisAppStale,
            FolderHandlerRules.Classify(new("open", Command(Exe), null, null), Exe));

    /// <summary>A command the default verb does not name is never invoked: with the stock
    /// <c>"none"</c> still in place the shell uses its own folder navigation and the registration
    /// sits there unread. Measured on a real machine — it is why this arm is stale, not
    /// registered.</summary>
    [Theory]
    [InlineData(null)]
    [InlineData("none")]
    public void ACommandTheDefaultVerbDoesNotNameIsStale(string? verb) =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisAppStale,
            FolderHandlerRules.Classify(new(verb, Command(Exe), verb, Command(Exe)), Exe));

    /// <summary>The exact wreckage the first version left: a default verb named with nothing behind
    /// it, which is what let the shell fall through to a third party's verb. It must not read as
    /// "unregistered", or the toggle would show off and leave it there.</summary>
    [Fact]
    public void ANamedVerbWithNoCommandIsNotMistakenForUnregistered() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToThisAppStale,
            FolderHandlerRules.Classify(new("open", null, "open", null), Exe));

    [Fact]
    public void AnotherFileManagerIsNeverMistakenForOurs() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToAnotherApp,
            FolderHandlerRules.Classify(
                new("open", ForeignCommand, "open", ForeignCommand),
                Exe));

    /// <summary>A half-and-half reading must not be repaired into ours: the foreign arm wins, so
    /// the self-heal leaves it alone rather than taking the other half from another program.</summary>
    [Fact]
    public void AForeignHalfWinsOverOurs() =>
        Assert.Equal(
            FolderHandlerState.RegisteredToAnotherApp,
            FolderHandlerRules.Classify(new("open", Command(Exe), "open", @"C:\Other\Opus.exe ""%1"""), Exe));

    /// <summary>
    /// <c>%1</c> on a drive root expands to <c>C:\</c>, and that trailing backslash escapes the
    /// closing quote of <c>"%1"</c> — so the shell hands <c>argv</c> a path with a quote stuck on
    /// the end. The parser already repairs it, which is why the registration can use the ordinary
    /// <c>"%1"</c> for drives as well as folders.
    /// </summary>
    [Fact]
    public void ADriveRootSurvivesTheQuoteItsTrailingBackslashEscapes()
    {
        var mangled = BertBrowser.Core.Cli.CommandLine.Parse([@"C:\"""]);

        Assert.Equal(@"C:\", Assert.Single(mangled.Targets).Path);
        Assert.Empty(mangled.Errors);
    }

    // --- what the startup repair may touch ---

    private static bool Missing(string _) => false;
    private static bool Present(string _) => true;

    /// <summary>The failure this rule exists for: the registered program is gone, so every folder
    /// double-click fails until something puts a live path back.</summary>
    [Fact]
    public void ARegistrationPointingAtSomethingGoneIsRepaired() =>
        Assert.True(FolderHandlerRules.ShouldRepair(Registered(@"D:\Old\BertBrowser.exe"), Exe, Missing));

    /// <summary>A debug build run beside a working install must not repoint the shell at
    /// <c>bin\Debug</c> — the installed exe is still there and still opens folders fine.</summary>
    [Fact]
    public void AnotherLiveInstallIsLeftAlone() =>
        Assert.False(FolderHandlerRules.ShouldRepair(Registered(@"D:\Old\BertBrowser.exe"), Exe, Present));

    /// <summary>Already pointing here, just incomplete — the drive half missing, or the default
    /// verb never written. Repairing that costs nothing and is what finishes a half-done write.</summary>
    [Fact]
    public void AHalfWrittenRegistrationForThisExeIsRepaired() =>
        Assert.True(FolderHandlerRules.ShouldRepair(new("open", Command(Exe), null, null), Exe, Present));

    [Fact]
    public void AnAbsentRegistrationIsNeverCreatedByTheRepair() =>
        Assert.False(FolderHandlerRules.ShouldRepair(FolderHandlerReading.None, Exe, Missing));

    /// <summary>Another program's registration is not ours to fix, even if its exe has gone.</summary>
    [Fact]
    public void AnotherAppsRegistrationIsNeverRepaired() =>
        Assert.False(FolderHandlerRules.ShouldRepair(new("open", ForeignCommand, "open", ForeignCommand), Exe, Missing));

    [Fact]
    public void AWorkingRegistrationIsNotRewrittenOnEveryLaunch() =>
        Assert.False(FolderHandlerRules.ShouldRepair(Registered(), Exe, Present));

    // --- reading a command line back ---

    [Theory]
    [InlineData(@"""C:\dir\BertBrowser.exe"" ""%1""", @"C:\dir\BertBrowser.exe")]
    [InlineData(@"C:\dir\app.exe ""%1""", @"C:\dir\app.exe")]
    [InlineData(@"C:\dir\app.exe", @"C:\dir\app.exe")]
    [InlineData(@"  ""C:\a b\app.exe"" %1  ", @"C:\a b\app.exe")]
    public void TheProgramTokenIsReadOffTheCommand(string command, string expected) =>
        Assert.Equal(expected, FolderHandlerRules.ProgramIn(command));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("\"unterminated")]
    public void AnUnusableCommandNamesNoProgram(string? command) =>
        Assert.Null(FolderHandlerRules.ProgramIn(command));

    [Fact]
    public void TheRegisteredProgramIsReportedForAMessage() =>
        Assert.Equal(
            ForeignProgram,
            FolderHandlerRules.RegisteredProgram(new("open", ForeignCommand, null, null)));
}
