using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class MetadataReadRulesTests
{
    [Theory]
    [InlineData(FileAttributes.Normal)]
    [InlineData(FileAttributes.Archive)]
    [InlineData(FileAttributes.Hidden)]
    [InlineData(FileAttributes.ReadOnly)]
    public void AnOrdinaryFileIsRead(FileAttributes attributes) =>
        Assert.True(MetadataReadRules.MayRead(attributes));

    /// <summary>
    /// The rule this class exists for. Reading a placeholder makes the sync provider fetch it, so a
    /// Dimensions column scrolled down a synced photo folder would download the whole folder a row
    /// at a time. Drop this and the theories below go green while the app quietly empties someone's
    /// OneDrive quota onto their disk.
    /// </summary>
    [Theory]
    [InlineData(FileAttributes.Offline)]
    public void ACloudPlaceholderIsNeverOpened(FileAttributes flag) =>
        Assert.False(MetadataReadRules.MayRead(FileAttributes.Normal | flag));

    [Fact]
    public void TheTwoRecallBitsDotNetDoesNotNameAreRefusedToo()
    {
        // Not in .NET's FileAttributes at all, which is why PreviewClassifier names them and why
        // this rule asks that class rather than writing the mask out a second time.
        Assert.False(MetadataReadRules.MayRead(FileAttributes.Normal | PreviewClassifier.RecallOnOpen));
        Assert.False(MetadataReadRules.MayRead(FileAttributes.Normal | PreviewClassifier.RecallOnDataAccess));
    }

    [Fact]
    public void AReparsePointIsRefusedRatherThanFollowed() =>
        Assert.False(MetadataReadRules.MayRead(FileAttributes.Normal | FileAttributes.ReparsePoint));

    [Fact]
    public void ADirectoryIsSkipped() =>
        Assert.False(MetadataReadRules.MayRead(FileAttributes.Directory));

    [Fact]
    public void ThePredicateIsSharedWithThePreviewPaneRatherThanReimplemented() =>
        // Said in that class's own words, so re-writing the mask here goes red.
        Assert.True(PreviewClassifier.IsCloudPlaceholder(FileAttributes.Offline));
}
