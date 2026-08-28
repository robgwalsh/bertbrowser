using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The diff behind live refresh. Its whole reason for existing is that rows which did not change
/// must be left alone — replacing them is what loses the selection, the scroll position and the
/// keyboard focus.
/// </summary>
public sealed class FileListDiffTests
{
    private static FileEntry Entry(
        string name, bool isDir = false, long size = 10, int minute = 0,
        FileAttributes attributes = FileAttributes.Normal) =>
        new(name, $@"C:\Dir\{name}", isDir, size,
            new DateTime(2026, 1, 1, 0, minute, 0, DateTimeKind.Utc), attributes);

    private static string Key(string name) => PathKey.Canonicalize($@"C:\Dir\{name}");

    [Fact]
    public void AnIdenticalListingIsNoChangeAtAll()
    {
        var listing = new[] { Entry("a.txt"), Entry("b.txt") };

        var changes = FileListDiff.Compute(listing, [Entry("a.txt"), Entry("b.txt")]);

        Assert.False(changes.Any);
    }

    [Fact]
    public void ANewFileIsAdded()
    {
        var changes = FileListDiff.Compute([Entry("a.txt")], [Entry("a.txt"), Entry("b.txt")]);

        Assert.Equal("b.txt", Assert.Single(changes.Added).Name);
        Assert.Empty(changes.Removed);
        Assert.Empty(changes.Updated);
    }

    [Fact]
    public void AVanishedFileIsRemoved()
    {
        var changes = FileListDiff.Compute([Entry("a.txt"), Entry("b.txt")], [Entry("a.txt")]);

        Assert.Equal(Key("b.txt"), Assert.Single(changes.Removed));
        Assert.Empty(changes.Added);
    }

    /// <summary>A rename is not a special case: the old path went and a new one arrived.</summary>
    [Fact]
    public void ARenameIsOneRemovalAndOneAddition()
    {
        var changes = FileListDiff.Compute([Entry("old.txt")], [Entry("new.txt")]);

        Assert.Equal(Key("old.txt"), Assert.Single(changes.Removed));
        Assert.Equal("new.txt", Assert.Single(changes.Added).Name);
    }

    [Theory]
    [InlineData(99, 0, FileAttributes.Normal)]                 // grew
    [InlineData(10, 5, FileAttributes.Normal)]                 // touched
    [InlineData(10, 0, FileAttributes.Hidden)]                 // hidden bit set
    public void ARowWhoseDetailsChangedIsUpdated(long size, int minute, FileAttributes attributes)
    {
        var changes = FileListDiff.Compute(
            [Entry("a.txt")], [Entry("a.txt", size: size, minute: minute, attributes: attributes)]);

        Assert.Equal("a.txt", Assert.Single(changes.Updated).Name);
        Assert.Empty(changes.Added);
        Assert.Empty(changes.Removed);
    }

    /// <summary>
    /// The case that would otherwise slip through. Path keys are uppercased, so a rename that only
    /// changes casing is the *same* key — comparing keys alone would report no change and leave the
    /// list showing the old spelling. Drop the ordinal name comparison and this goes red.
    /// </summary>
    [Fact]
    public void ACaseOnlyRenameIsAnUpdate_NotNothing()
    {
        var changes = FileListDiff.Compute([Entry("notes.txt")], [Entry("Notes.txt")]);

        Assert.Equal("Notes.txt", Assert.Single(changes.Updated).Name);
        Assert.Empty(changes.Added);
        Assert.Empty(changes.Removed);
    }

    /// <summary>A file replaced by a folder of the same name is still a change worth showing.</summary>
    [Fact]
    public void AFileBecomingAFolderIsAnUpdate()
    {
        var changes = FileListDiff.Compute([Entry("thing")], [Entry("thing", isDir: true)]);

        Assert.True(Assert.Single(changes.Updated).IsDirectory);
    }

    [Fact]
    public void AnEmptiedFolderRemovesEverything()
    {
        var changes = FileListDiff.Compute([Entry("a.txt"), Entry("b.txt")], []);

        Assert.Equal(2, changes.Removed.Count);
        Assert.True(changes.Any);
    }

    [Fact]
    public void AFolderFillingFromEmptyAddsEverything()
    {
        var changes = FileListDiff.Compute([], [Entry("a.txt"), Entry("b.txt")]);

        Assert.Equal(2, changes.Added.Count);
    }

    /// <summary>Comparison is by canonical key, so the same file listed with different casing is
    /// the same file — not a removal plus an addition.</summary>
    [Fact]
    public void PathsAreComparedCanonically()
    {
        var before = new[] { new FileEntry("a.txt", @"C:\Dir\a.txt", false, 10, default, FileAttributes.Normal) };
        var after = new[] { new FileEntry("a.txt", @"c:\DIR\a.txt", false, 10, default, FileAttributes.Normal) };

        Assert.False(FileListDiff.Compute(before, after).Any);
    }
}
