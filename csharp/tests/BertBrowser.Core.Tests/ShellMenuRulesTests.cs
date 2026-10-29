using BertBrowser.Core.Services.ShellMenu;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// How raw registrations become the rows a user can tick, and how a handler's menu is tidied.
/// Nothing here opens a key: the App reads, this decides.
/// </summary>
public sealed class ShellMenuRulesTests
{
    private static readonly Guid SevenZip = new("23170F69-40C1-278A-1000-000100020000");
    private static readonly Guid Tortoise = new("30351346-7B7D-4FCC-81B4-1E394CA267EB");

    private static readonly IReadOnlySet<Guid> NothingBlocked = new HashSet<Guid>();

    private static ShellHandlerRegistration Handler(string family, Guid clsid, string name = "7-Zip", string? className = null) =>
        new(family, name, clsid, className);

    private static ShellVerbRegistration Verb(
        string family, string verb, string? name = null, string? command = "\"C:\\tool.exe\" \"%1\"",
        bool extended = false, bool legacyDisable = false, bool programmaticOnly = false,
        bool delegateExecute = false, bool subCommands = false) =>
        new(family, verb, name, null, command, extended, legacyDisable, programmaticOnly, delegateExecute, subCommands);

    // --- merging ---

    [Fact]
    public void AHandlerRegisteredUnderFourFamiliesIsOneRow()
    {
        var rows = ShellMenuRules.Catalog(
            [
                Handler("*", SevenZip), Handler("Directory", SevenZip),
                Handler("Folder", SevenZip), Handler("Drive", SevenZip),
            ],
            [], NothingBlocked);

        var row = Assert.Single(rows);
        Assert.Equal("clsid:{23170f69-40c1-278a-1000-000100020000}", row.Id);
        Assert.Equal("7-Zip", row.Name);
        Assert.Equal(ShellExtensionKind.Handler, row.Kind);
        Assert.Equal(SevenZip, row.Clsid);
        Assert.Equal(["*", "Directory", "Folder", "Drive"], row.Families);
    }

    [Fact]
    public void AVerbRegisteredForItemsAndBackgroundIsOneRowCarryingBoth()
    {
        var rows = ShellMenuRules.Catalog(
            [],
            [
                Verb("Directory", "git_shell", "Open Git Bash here", "\"C:\\Git\\git-bash.exe\" \"--cd=%1\""),
                Verb(@"Directory\Background", "git_shell", "Open Git Bash here", "\"C:\\Git\\git-bash.exe\" \"--cd=%v.\""),
            ],
            NothingBlocked);

        var row = Assert.Single(rows);
        Assert.Equal("verb:git_shell", row.Id);
        Assert.Equal("Open Git Bash here", row.Name);
        Assert.Equal(2, row.Verbs.Count);
        Assert.Equal(["Directory", @"Directory\Background"], row.Families);
    }

    [Fact]
    public void VerbsComeBeforeHandlersEachInRegistrationOrder()
    {
        var rows = ShellMenuRules.Catalog(
            [Handler("*", Tortoise, "TortoiseSVN"), Handler("*", SevenZip, "7-Zip")],
            [Verb("*", "b_verb", "B"), Verb("*", "a_verb", "A")],
            NothingBlocked);

        Assert.Equal(["B", "A", "TortoiseSVN", "7-Zip"], rows.Select(r => r.Name));
    }

    // --- naming ---

    [Fact]
    public void AHandlerNamedByItsClsidTakesTheClassName()
    {
        var rows = ShellMenuRules.Catalog(
            [Handler("*", SevenZip, SevenZip.ToString("B"), "7-Zip Shell Extension")], [], NothingBlocked);

        Assert.Equal("7-Zip Shell Extension", Assert.Single(rows).Name);
    }

    [Fact]
    public void AHandlerWithNoNameAnywhereShowsItsClsid()
    {
        var rows = ShellMenuRules.Catalog([Handler("*", SevenZip, "", null)], [], NothingBlocked);

        Assert.Equal("{23170F69-40C1-278A-1000-000100020000}", Assert.Single(rows).Name);
    }

    /// <summary>A canonical verb like <c>print</c> is registered lowercase and named nowhere;
    /// Explorer shows it from its own string table, and this at least does not show it lowercase.</summary>
    [Theory]
    [InlineData("Sublime", "Sublime")]
    [InlineData("print", "Print")]
    public void AVerbWithNoDisplayNameIsNamedByItsVerb(string verb, string expected)
    {
        var rows = ShellMenuRules.Catalog([], [Verb("*", verb)], NothingBlocked);

        Assert.Equal(expected, Assert.Single(rows).Name);
    }

    [Fact]
    public void AVerbTakesItsNameFromWhicheverFamilyHasOne()
    {
        var rows = ShellMenuRules.Catalog(
            [], [Verb("Directory", "x"), Verb(@"Directory\Background", "x", "Do X here")], NothingBlocked);

        Assert.Equal("Do X here", Assert.Single(rows).Name);
    }

    // --- what is never offered ---

    [Fact]
    public void ABlockedHandlerIsDropped()
    {
        var rows = ShellMenuRules.Catalog(
            [Handler("*", SevenZip), Handler("*", Tortoise, "TortoiseSVN")],
            [], new HashSet<Guid> { Tortoise });

        Assert.Equal("7-Zip", Assert.Single(rows).Name);
    }

    /// <summary>Open With, as Windows registers it under <c>*\shellex\ContextMenuHandlers</c>.
    /// The app has its own Open; Explorer's stock items must not come back in through the side door.</summary>
    [Fact]
    public void WindowsOwnHandlerForAVerbTheAppHasIsDropped()
    {
        var openWith = new Guid("09799AFB-AD67-11D1-ABCD-00C04FC30936");

        var rows = ShellMenuRules.Catalog(
            [Handler("*", openWith, "Open With"), Handler("*", SevenZip)], [], NothingBlocked);

        Assert.Equal("7-Zip", Assert.Single(rows).Name);
    }

    [Theory]
    [InlineData("open")]
    [InlineData("Open")]
    [InlineData("explore")]
    [InlineData("runas")]
    [InlineData("cmd")]
    [InlineData("Powershell")]
    [InlineData("printto")]
    public void AStockVerbTheAppAlreadyHasIsDropped(string verb) =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", verb, "Whatever")], NothingBlocked));

    [Theory]
    [InlineData("edit")]
    [InlineData("print")]
    public void AFileTypesOwnVerbsStay(string verb) =>
        Assert.Single(ShellMenuRules.Catalog([], [Verb("txtfile", verb, "Edit")], NothingBlocked));

    [Fact]
    public void AnExtendedVerbIsDropped() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", extended: true)], NothingBlocked));

    [Fact]
    public void ALegacyDisabledVerbIsDropped() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", legacyDisable: true)], NothingBlocked));

    [Fact]
    public void AProgrammaticOnlyVerbIsDropped() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", programmaticOnly: true)], NothingBlocked));

    [Fact]
    public void ADelegateExecuteVerbIsDroppedBecauseItsCommandLineIsNotWhatRuns() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", delegateExecute: true)], NothingBlocked));

    [Fact]
    public void ACascadingVerbIsDropped() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", subCommands: true)], NothingBlocked));

    [Fact]
    public void AVerbWithNoCommandIsDropped() =>
        Assert.Empty(ShellMenuRules.Catalog([], [Verb("*", "x", "X", command: null)], NothingBlocked));

    // --- the user's choice ---

    [Fact]
    public void TheMasterSwitchOffHidesEverything()
    {
        var catalog = ShellMenuRules.Catalog([Handler("*", SevenZip)], [], NothingBlocked);

        Assert.Empty(ShellMenuRules.Visible(catalog, enabled: false, []));
    }

    [Fact]
    public void AHiddenIdIsLeftOutCaseInsensitively()
    {
        var catalog = ShellMenuRules.Catalog(
            [Handler("*", SevenZip), Handler("*", Tortoise, "TortoiseSVN")], [Verb("*", "git_shell", "Git")],
            NothingBlocked);

        var visible = ShellMenuRules.Visible(catalog, enabled: true, ["VERB:GIT_SHELL", ShellExtension.HandlerId(Tortoise)]);

        Assert.Equal("7-Zip", Assert.Single(visible).Name);
    }

    [Fact]
    public void SavingKeepsHiddenIdsThatWereNotFoundThisTime()
    {
        var hidden = ShellMenuRules.HiddenAfterSave(
            ["verb:uninstalled", "verb:git_shell"],
            [("verb:git_shell", true), ("clsid:{x}", false)]);

        Assert.Equal(["verb:uninstalled", "clsid:{x}"], hidden);
    }

    // --- menu text ---

    [Theory]
    [InlineData("&Add to archive...", "_Add to archive...")]
    [InlineData("Tom && Jerry", "Tom & Jerry")]
    [InlineData("snake_case", "snake__case")]
    [InlineData("Extract\tCtrl+E", "Extract")]
    [InlineData("", "")]
    [InlineData(null, "")]
    public void AWin32MenuStringBecomesAWpfHeader(string? win32, string expected) =>
        Assert.Equal(expected, ShellMenuRules.Header(win32));

    // --- tidying a handler's menu ---

    [Fact]
    public void SeparatorsAtTheEndsAndDoubledUpAreCollapsed()
    {
        var tidy = ShellMenuRules.Tidy(
        [
            ShellMenuEntry.Separator,
            ShellMenuEntry.Item("A", 1),
            ShellMenuEntry.Separator,
            ShellMenuEntry.Separator,
            ShellMenuEntry.Item("B", 2),
            ShellMenuEntry.Separator,
        ]);

        Assert.Equal(["A", "", "B"], tidy.Select(e => e.Header));
        Assert.True(tidy[1].IsSeparator);
    }

    [Fact]
    public void AnEmptySubmenuAndANamelessItemAreDropped()
    {
        var tidy = ShellMenuRules.Tidy(
        [
            ShellMenuEntry.Submenu("Empty", []),
            ShellMenuEntry.Submenu("Only separators", [ShellMenuEntry.Separator]),
            ShellMenuEntry.Item("", 7),
            ShellMenuEntry.Item("Kept", 8),
        ]);

        Assert.Equal("Kept", Assert.Single(tidy).Header);
    }

    [Fact]
    public void ASubmenuIsTidiedInside()
    {
        var tidy = ShellMenuRules.Tidy(
        [
            ShellMenuEntry.Submenu("7-Zip", [ShellMenuEntry.Separator, ShellMenuEntry.Item("Extract", 1), ShellMenuEntry.Separator]),
        ]);

        var submenu = Assert.Single(tidy);
        Assert.Equal("Extract", Assert.Single(submenu.Children).Header);
    }
}
