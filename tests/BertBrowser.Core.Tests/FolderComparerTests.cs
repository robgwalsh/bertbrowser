using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Pairing two trees and folding each folder's verdict up from what is beneath it. The fold is what
/// lets a whole subtree be synced without opening it, which makes it the piece a wrong answer is
/// hardest to notice in: a folder that says "same" is a folder nobody looks inside.
/// </summary>
public sealed class FolderComparerTests
{
    private static readonly DateTime Noon = new(2026, 3, 14, 12, 0, 0, DateTimeKind.Utc);

    private static CompareEntry File_(string display, long size = 10, DateTime? modified = null) =>
        new(display.ToUpperInvariant(), display, Path.GetFileName(display), false, size, modified ?? Noon);

    private static CompareEntry Folder(string display) =>
        new(display.ToUpperInvariant(), display, Path.GetFileName(display), true, 0, Noon);

    private static CompareResult Compare(CompareEntry[] left, CompareEntry[] right) =>
        FolderComparer.Compare(left, right, CompareTolerance.Strict);

    // --- Pairing ---

    [Fact]
    public void TwoIdenticalTreesHoldNoDifferences()
    {
        var tree = new[] { Folder("src"), File_(@"src\main.cs"), File_("readme.md") };

        var result = Compare(tree, tree);

        Assert.False(result.AnyDifference);
        Assert.Equal(3, result.SameCount);
        Assert.Equal(0, result.UnknownCount);
    }

    [Fact]
    public void TwoEmptyTreesHoldNoDifferences() =>
        Assert.False(Compare([], []).AnyDifference);

    [Fact]
    public void EachSideKeepsWhatOnlyItHas()
    {
        var result = Compare([File_("a.txt")], [File_("b.txt")]);

        Assert.Equal(CompareVerdict.LeftOnly, result.For("A.TXT"));
        Assert.Equal(CompareVerdict.RightOnly, result.For("B.TXT"));
    }

    /// <summary>Windows does not have two files whose names differ only in case, and neither does
    /// the key: the pair must meet, or a copy would land beside its own original.</summary>
    [Fact]
    public void NamesDifferingOnlyInCaseArePairedTogether()
    {
        var result = Compare([File_("Notes.txt")], [File_("notes.TXT")]);

        Assert.Equal(CompareVerdict.Same, result.For("NOTES.TXT"));
        Assert.Equal(1, result.SameCount);
    }

    [Fact]
    public void APathTheComparisonNeverSawIsUnknown() =>
        Assert.Equal(CompareVerdict.Unknown, Compare([], []).For(@"SRC\LATER.CS"));

    // --- The fold ---

    [Fact]
    public void AFolderWhoseContentsAllMatchIsSame()
    {
        var tree = new[] { Folder("src"), File_(@"src\a.cs"), File_(@"src\b.cs") };

        Assert.Equal(CompareVerdict.Same, Compare(tree, tree).For("SRC"));
    }

    [Fact]
    public void OneChangedFileMakesEveryFolderAboveItDiffer()
    {
        var left = new[] { Folder("src"), Folder(@"src\deep"), File_(@"src\deep\a.cs", size: 10) };
        var right = new[] { Folder("src"), Folder(@"src\deep"), File_(@"src\deep\a.cs", size: 99) };

        var result = Compare(left, right);

        Assert.Equal(CompareVerdict.Differs, result.For("SRC"));
        Assert.Equal(CompareVerdict.Differs, result.For(@"SRC\DEEP"));
    }

    /// <summary>
    /// The load-bearing one. A folder that says "same" is a folder nobody opens, and "same" is what
    /// authorises deleting the other side of it — so one descendant nothing is known about has to
    /// stop the whole subtree being called a match.
    /// </summary>
    [Fact]
    public void OneUnknownDescendantMakesTheWholeSubtreeUnknown()
    {
        var left = new[]
        {
            Folder("src"), Folder(@"src\deep"),
            File_(@"src\a.cs"), File_(@"src\deep\b.cs", modified: DateTime.MinValue),
        };

        var result = Compare(left, left);

        Assert.Equal(CompareVerdict.Unknown, result.For(@"SRC\DEEP\B.CS"));
        Assert.Equal(CompareVerdict.Unknown, result.For(@"SRC\DEEP"));
        Assert.Equal(CompareVerdict.Unknown, result.For("SRC"));
        Assert.Equal(CompareVerdict.Same, result.For(@"SRC\A.CS"));
    }

    /// <summary>
    /// A folder missing from the right is missing wholesale; calling it merely "different" would
    /// have the sync copy its contents into a folder that is not there.
    /// </summary>
    [Fact]
    public void AFolderMissingOnOneSideStaysMissing_NotMerelyDifferent()
    {
        var left = new[] { Folder("bin"), File_(@"bin\app.exe") };

        var result = Compare(left, []);

        Assert.Equal(CompareVerdict.LeftOnly, result.For("BIN"));
        Assert.Equal(CompareVerdict.LeftOnly, result.For(@"BIN\APP.EXE"));
    }

    /// <summary>
    /// The inverse, and the mistake a naive fold makes: one new file inside a folder that exists on
    /// both sides must not rename the folder "left only".
    /// </summary>
    [Fact]
    public void ANewFileInsideASharedFolderLeavesTheFolderShared()
    {
        var left = new[] { Folder("src"), File_(@"src\a.cs"), File_(@"src\new.cs") };
        var right = new[] { Folder("src"), File_(@"src\a.cs") };

        Assert.Equal(CompareVerdict.Differs, Compare(left, right).For("SRC"));
    }

    [Fact]
    public void AFileOnOneSideAndAFolderOnTheOtherDiffers()
    {
        var result = Compare([File_("thing")], [Folder("thing"), File_(@"thing\inner.txt")]);

        Assert.Equal(CompareVerdict.Differs, result.For("THING"));
    }

    /// <summary>A listing that carries a file but not its parent folder row — which the index can
    /// do — still gets a verdict for the folder, decided by what is under it.</summary>
    [Fact]
    public void AFolderWithNoRowOfItsOwnIsStillJudgedByItsContents()
    {
        var result = Compare([File_(@"src\a.cs", size: 10)], [File_(@"src\a.cs", size: 99)]);

        Assert.Equal(CompareVerdict.Differs, result.For("SRC"));
    }

    // --- Counts ---

    [Fact]
    public void TheCountsCoverEveryPathEitherSideHolds()
    {
        var left = new[]
        {
            File_("same.txt"), File_("gone.txt"), File_("odd.txt", modified: DateTime.MinValue),
        };
        var right = new[] { File_("same.txt"), File_("odd.txt", modified: DateTime.MinValue) };

        var result = Compare(left, right);

        Assert.Equal(1, result.SameCount);
        Assert.Equal(1, result.UnknownCount);
        Assert.Equal(1, result.DifferenceCount);
        Assert.True(result.AnyDifference);
    }

    /// <summary>
    /// Presence is answered before timestamps are. A file the other side simply does not have is
    /// "only here" whatever its own timestamp says — there is nothing to be uncertain about, and
    /// calling it unknown would quietly refuse to copy it.
    /// </summary>
    [Fact]
    public void AMissingTimestampOnAnUnpairedFileIsStillOnlyOnOneSide()
    {
        var result = Compare([File_("odd.txt", modified: DateTime.MinValue)], []);

        Assert.Equal(CompareVerdict.LeftOnly, result.For("ODD.TXT"));
    }
}
