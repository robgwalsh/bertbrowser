using BertBrowser.Core.Services.ShellMenu;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Which registry key families are asked for a selection's context-menu entries. The list has to
/// match Explorer's, or an extension that shows there is missing here for no reason a user could
/// find.
/// </summary>
public sealed class ShellMenuKeysTests
{
    private static ShellFileType? NoType(string extension) => null;

    private static ShellFileType? TextType(string extension) =>
        extension == ".txt" ? new ShellFileType("txtfile", "text") : null;

    [Fact]
    public void AFileAsksItsProgIdItsExtensionItsPerceivedTypeThenEverything()
    {
        var keys = ShellMenuKeys.For(
            [new ShellMenuTarget(@"C:\docs\notes.txt", false)], ShellMenuContext.Items, TextType);

        Assert.Equal(
            ["txtfile", @"SystemFileAssociations\.txt", @"SystemFileAssociations\text", "*", "AllFilesystemObjects"],
            keys);
    }

    [Fact]
    public void AFileOfUnknownTypeStillAsksTheGenericFamilies()
    {
        var keys = ShellMenuKeys.For(
            [new ShellMenuTarget(@"C:\docs\thing.xyz", false)], ShellMenuContext.Items, NoType);

        Assert.Equal([@"SystemFileAssociations\.xyz", "*", "AllFilesystemObjects"], keys);
    }

    [Fact]
    public void AFileWithNoExtensionAsksOnlyTheGenericFamilies()
    {
        var keys = ShellMenuKeys.For(
            [new ShellMenuTarget(@"C:\docs\README", false)], ShellMenuContext.Items, NoType);

        Assert.Equal(["*", "AllFilesystemObjects"], keys);
    }

    [Fact]
    public void AFolderAsksDirectoryAndFolder()
    {
        var keys = ShellMenuKeys.For(
            [new ShellMenuTarget(@"C:\docs\photos", true)], ShellMenuContext.Items, NoType);

        Assert.Equal(["Directory", "Folder", "AllFilesystemObjects"], keys);
    }

    [Theory]
    [InlineData(@"D:\")]
    [InlineData(@"D:")]
    public void ADriveRootAsksDriveInsteadOfDirectory(string root)
    {
        var keys = ShellMenuKeys.For([new ShellMenuTarget(root, true)], ShellMenuContext.Items, NoType);

        Assert.Equal(["Drive", "Folder", "AllFilesystemObjects"], keys);
    }

    /// <summary>Explorer asks about the first item's type and lets the handlers see the rest through
    /// the data object; a mixed selection therefore follows whatever came first.</summary>
    [Fact]
    public void AMixedSelectionFollowsTheFirstItem()
    {
        var keys = ShellMenuKeys.For(
            [new ShellMenuTarget(@"C:\docs\photos", true), new ShellMenuTarget(@"C:\docs\notes.txt", false)],
            ShellMenuContext.Items, TextType);

        Assert.Equal(["Directory", "Folder", "AllFilesystemObjects"], keys);
    }

    [Fact]
    public void TheBackgroundIsItsOwnFamilyAndNothingElse()
    {
        var keys = ShellMenuKeys.For([], ShellMenuContext.Background, TextType);

        Assert.Equal([@"Directory\Background"], keys);
    }

    [Fact]
    public void NoItemsMeansNoKeys() =>
        Assert.Empty(ShellMenuKeys.For([], ShellMenuContext.Items, NoType));
}
