using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The naming rule the rename dialog previews and the rename itself obeys: one item takes the typed
/// name whole, several are numbered while keeping their own extensions.
/// </summary>
public sealed class RenamePatternTests
{
    private static RenameSource File_(string path) => new(path, IsDirectory: false);

    private static RenameSource Dir(string path) => new(path, IsDirectory: true);

    // --- one item takes the name as typed ---

    [Fact]
    public void OneItem_TakesThePatternAsItsWholeName()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\old.txt")], "notes.txt");

        Assert.Equal(["notes.txt"], names);
    }

    [Fact]
    public void OneItem_KeepsWhateverExtensionWasTyped_IncludingNone()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\old.txt")], "notes");

        Assert.Equal(["notes"], names);
    }

    [Fact]
    public void SurroundingSpaceIsDropped()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\old.txt")], "  notes.txt  ");

        Assert.Equal(["notes.txt"], names);
    }

    [Fact]
    public void TrailingDotsAreDropped_BecauseWindowsDropsThemAnyway()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\old.txt")], "notes.");

        Assert.Equal(["notes"], names);
    }

    // --- several items are numbered ---

    [Fact]
    public void SeveralItems_AreNumberedFromOne_AndKeepTheirOwnExtensions()
    {
        var names = RenamePattern.Apply(
            [File_(@"C:\p\a.jpg"), File_(@"C:\p\b.png"), File_(@"C:\p\c.jpg")], "Holiday");

        Assert.Equal(["Holiday 1.jpg", "Holiday 2.png", "Holiday 3.jpg"], names);
    }

    [Fact]
    public void SeveralItems_NumberInTheOrderGiven()
    {
        var names = RenamePattern.Apply(
            [File_(@"C:\p\z.txt"), File_(@"C:\p\a.txt")], "Note");

        Assert.Equal(["Note 1.txt", "Note 2.txt"], names);
    }

    [Fact]
    public void AFolderInABatch_GetsNoExtension()
    {
        // "My.Project" is a folder, so ".Project" is part of its name, not an extension to keep.
        var names = RenamePattern.Apply([Dir(@"C:\p\My.Project"), Dir(@"C:\p\Other")], "Work");

        Assert.Equal(["Work 1", "Work 2"], names);
    }

    [Fact]
    public void AFileWithNoExtension_IsJustNumbered()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\LICENSE"), File_(@"C:\p\a.txt")], "Doc");

        Assert.Equal(["Doc 1", "Doc 2.txt"], names);
    }

    [Fact]
    public void NoItems_ProducesNoNames() =>
        Assert.Empty(RenamePattern.Apply([], "anything"));

    // --- what the dialog starts with ---

    [Fact]
    public void Suggestion_ForOneFile_IsItsWholeName() =>
        Assert.Equal("report.pdf", RenamePattern.SuggestFor([File_(@"C:\p\report.pdf")]));

    [Fact]
    public void Suggestion_ForSeveralFiles_DropsTheExtension_BecauseNumberingReaddsIt() =>
        Assert.Equal("report",
            RenamePattern.SuggestFor([File_(@"C:\p\report.pdf"), File_(@"C:\p\other.pdf")]));

    [Fact]
    public void Suggestion_ForSeveralFolders_KeepsTheWholeName() =>
        Assert.Equal("My.Project",
            RenamePattern.SuggestFor([Dir(@"C:\p\My.Project"), Dir(@"C:\p\Other")]));

    [Fact]
    public void BaseNameLength_CoversTheStemOnly() =>
        Assert.Equal(6, RenamePattern.BaseNameLength(File_(@"C:\p\report.pdf")));

    [Fact]
    public void BaseNameLength_OfADotfile_CoversTheWholeName() =>
        Assert.Equal(10, RenamePattern.BaseNameLength(File_(@"C:\p\.gitignore")));

    [Fact]
    public void BaseNameLength_OfAFolder_CoversTheWholeName() =>
        Assert.Equal(10, RenamePattern.BaseNameLength(Dir(@"C:\p\My.Project")));

    // --- validation ---

    [Theory]
    [InlineData("notes.txt")]
    [InlineData(".gitignore")]
    [InlineData("a name with spaces.md")]
    [InlineData("CONTENTS.txt")] // only the exact device name is reserved
    public void LegalNames_Validate(string name) => Assert.Null(RenamePattern.Validate(name));

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("a/b.txt")]
    [InlineData(@"a\b.txt")]
    [InlineData("a:b.txt")]
    [InlineData("a*b.txt")]
    [InlineData("a?b.txt")]
    [InlineData("a\"b.txt")]
    [InlineData("a<b.txt")]
    [InlineData("a|b.txt")]
    [InlineData("trailing.")]
    [InlineData("trailing ")]
    [InlineData("CON")]
    [InlineData("con.txt")]
    [InlineData("LPT1.log")]
    public void IllegalNames_AreRejectedWithAReason(string name) =>
        Assert.False(string.IsNullOrWhiteSpace(RenamePattern.Validate(name)));

    [Fact]
    public void OverlongNames_AreRejected() =>
        Assert.NotNull(RenamePattern.Validate(new string('a', RenamePattern.MaxNameLength + 1)));

    [Fact]
    public void ANameAtTheLimit_IsAccepted() =>
        Assert.Null(RenamePattern.Validate(new string('a', RenamePattern.MaxNameLength)));

    // --- the plain box is unchanged, including where it was never covered ---
    //
    // These are the cases the advanced-rename engine could have broken silently: every multi-item
    // test above uses an already-clean pattern, and none of them types a brace.

    [Fact]
    public void ADirtyPattern_IsCleanedBeforeNumbering_NotAfter()
    {
        // Cleaning the finished name instead would give "Holiday   1.jpg": the trim cannot reach
        // the middle, because by then the name ends in ".jpg".
        var names = RenamePattern.Apply([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.jpg")], "  Holiday  ");

        Assert.Equal(["Holiday 1.jpg", "Holiday 2.jpg"], names);
    }

    [Fact]
    public void ATrailingDot_IsDroppedBeforeNumbering()
    {
        var names = RenamePattern.Apply([File_(@"C:\p\a.txt"), File_(@"C:\p\b.txt")], "notes.");

        Assert.Equal(["notes 1.txt", "notes 2.txt"], names);
    }

    [Fact]
    public void BracesAreTakenLiterally_BecauseTheyAreLegalInAName()
    {
        // "{6B99A0C1}.tmp" is an ordinary name to give a file and the plain box has always
        // accepted it. Tokens belong to the expanded panel and nowhere else.
        var names = RenamePattern.Apply([File_(@"C:\p\old.tmp")], "{6B99A0C1}.tmp");

        Assert.Equal(["{6B99A0C1}.tmp"], names);
    }

    [Fact]
    public void AnEmptyPatternOverSeveralItems_ProducesNamesThatAreRefused()
    {
        // Numbering an empty pattern gives " 1.jpg", which Validate accepts — so a box the user
        // had merely cleared would leave Rename enabled.
        var names = RenamePattern.Apply([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.jpg")], "");

        Assert.All(names, name => Assert.NotNull(RenamePattern.Validate(name)));
    }

    // --- the stem/extension split the scopes and case transforms are built on ---

    [Fact]
    public void Split_OfAFile_SeparatesTheExtension() =>
        Assert.Equal(("report", ".pdf"), RenamePattern.Split(File_(@"C:\p\report.pdf")));

    [Fact]
    public void Split_OfAFolder_KeepsTheWholeName_BecauseAFolderHasNoExtension() =>
        Assert.Equal(("My.Project", ""), RenamePattern.Split(Dir(@"C:\p\My.Project")));

    [Fact]
    public void Split_OfADotfile_KeepsTheWholeName()
    {
        // Path calls ".gitignore" all extension and no stem. Taking that literally would leave a
        // stem-scoped find/replace nothing to work on, and let an extension-scoped one rewrite
        // the entire filename.
        Assert.Equal((".gitignore", ""), RenamePattern.Split(File_(@"C:\p\.gitignore")));
    }
}
