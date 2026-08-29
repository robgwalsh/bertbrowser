using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Duplicates;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The judgements a duplicate scan makes before it shows or removes anything. All pure, so they are
/// held still here rather than eyeballed in a window that takes minutes to fill.
/// </summary>
public sealed class DuplicateRulesTests
{
    private static DuplicateFile File_(string path, DateTime? modified = null, long size = 4096) =>
        new(path, "", Path.GetFileName(path), size, modified ?? new DateTime(2024, 3, 14, 9, 26, 53), false);

    private static DuplicateGroup Group(params DuplicateFile[] files) =>
        new(files[0].SizeBytes, "HASH", files);

    // --- Classify ---

    /// <summary>
    /// The one that matters. The FSCTL_ENUM_USN_DATA build path writes every row with
    /// size_bytes = 0, so on such a volume every file collides with every other and the shortlist
    /// means nothing at all — a scan that trusted it would try to hash the whole disk. Rows in
    /// scope with not one real length is the signature, and it is not the same thing as a disk with
    /// no duplicates on it. Make Classify return Ready here and this goes red.
    /// </summary>
    [Fact]
    public void RowsButNoLengths_IsNotSizeData_NotAnEmptyResult()
    {
        var verdict = DuplicateRules.Classify(
            filesInScope: 500, sizedFilesInScope: 0, isBuilding: false, isIndexed: true);

        Assert.Equal(DuplicateScanAvailability.NoSizeData, verdict);
    }

    [Fact]
    public void RealLengthsInScope_AreReady()
    {
        Assert.Equal(
            DuplicateScanAvailability.Ready,
            DuplicateRules.Classify(500, 500, isBuilding: false, isIndexed: true));
    }

    /// <summary>
    /// A network share or a removable disk is filled in by IndexCrawler rather than the MFT pass,
    /// so the index service says "not indexed" about rows whose sizes are perfectly good. The
    /// evidence has to beat the opinion, or the feature refuses to scan folders it can scan.
    /// </summary>
    [Fact]
    public void RealLengths_BeatTheIndexServiceSayingNo()
    {
        Assert.Equal(
            DuplicateScanAvailability.Ready,
            DuplicateRules.Classify(40, 40, isBuilding: false, isIndexed: false));
    }

    [Fact]
    public void StillBuilding_WithLengths_SaysSo()
    {
        Assert.Equal(
            DuplicateScanAvailability.Building,
            DuplicateRules.Classify(40, 40, isBuilding: true, isIndexed: false));
    }

    /// <summary>A volume that has finished is Ready even while another one is still going.</summary>
    [Fact]
    public void StillBuildingElsewhere_ButThisScopeIsDone_IsReady()
    {
        Assert.Equal(
            DuplicateScanAvailability.Ready,
            DuplicateRules.Classify(40, 40, isBuilding: true, isIndexed: true));
    }

    [Fact]
    public void NothingIndexedAndNothingInScope_IsNotIndexed()
    {
        Assert.Equal(
            DuplicateScanAvailability.NotIndexed,
            DuplicateRules.Classify(0, 0, isBuilding: false, isIndexed: false));
    }

    /// <summary>
    /// An indexed folder that really is empty has been looked at and found to hold nothing, which
    /// is an answer. Reporting NotIndexed there would send the user to a retry button that cannot
    /// change anything.
    /// </summary>
    [Fact]
    public void AnIndexedButEmptyScope_IsReady_NotUnindexed()
    {
        Assert.Equal(
            DuplicateScanAvailability.Ready,
            DuplicateRules.Classify(0, 0, isBuilding: false, isIndexed: true));
    }

    [Fact]
    public void NothingInScopeWhileBuilding_IsBuilding()
    {
        Assert.Equal(
            DuplicateScanAvailability.Building,
            DuplicateRules.Classify(0, 0, isBuilding: true, isIndexed: false));
    }

    // --- system subtrees ---

    [Fact]
    public void InsideWindows_IsASystemSubtree()
    {
        var windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows).ToUpperInvariant();
        Assert.True(DuplicateRules.IsSystemSubtree($@"{windows}\WINSXS\SOMETHING.DLL"));
    }

    /// <summary>
    /// The profile root is in ProtectedLocations and deliberately not here. It is where a person's
    /// duplicates actually live — downloads saved twice, photos imported twice — so skipping it
    /// would leave this feature with nothing to find.
    /// </summary>
    [Fact]
    public void TheProfileRoot_IsNotASystemSubtree()
    {
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile).ToUpperInvariant();
        Assert.False(DuplicateRules.IsSystemSubtree($@"{profile}\DOWNLOADS\REPORT.PDF"));
    }

    /// <summary>Held paths go whether or not system folders are being skipped: those files have
    /// been deleted as far as the user is concerned, and offering to delete one again reads as a
    /// delete that silently failed.</summary>
    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void AHeldPath_IsAlwaysExcluded(bool skipSystemFolders)
    {
        Assert.True(DuplicateRules.IsExcluded(
            @"C:\.BERTBROWSER-TRASH\DELETE-ABC\REPORT.PDF", skipSystemFolders));
    }

    [Fact]
    public void SystemFoldersAreOnlyExcludedWhenAsked()
    {
        var windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows).ToUpperInvariant();
        var path = $@"{windows}\WINSXS\SOMETHING.DLL";

        Assert.True(DuplicateRules.IsExcluded(path, skipSystemFolders: true));
        Assert.False(DuplicateRules.IsExcluded(path, skipSystemFolders: false));
    }

    // --- CanRemove ---

    /// <summary>
    /// The guard the whole destructive half rests on. The point of the feature is to reclaim what a
    /// redundant copy costs, so one copy always stays; a batch that took all of them would destroy
    /// the only remaining instance of a file the user was just told they had several of.
    /// </summary>
    [Fact]
    public void EveryCopyTicked_IsRefused()
    {
        Assert.False(DuplicateRules.CanRemove(groupSize: 3, tickedCount: 3));
    }

    [Fact]
    public void AllButOneTicked_IsAllowed()
    {
        Assert.True(DuplicateRules.CanRemove(groupSize: 3, tickedCount: 2));
    }

    [Fact]
    public void NothingTicked_IsNotWork()
    {
        Assert.False(DuplicateRules.CanRemove(groupSize: 3, tickedCount: 0));
    }

    // --- ChooseKeeper ---

    [Fact]
    public void Newest_KeepsTheLatestModified()
    {
        var group = Group(
            File_(@"C:\a\one.bin", new DateTime(2020, 1, 1)),
            File_(@"C:\b\two.bin", new DateTime(2024, 6, 1)),
            File_(@"C:\c\three.bin", new DateTime(2022, 1, 1)));

        Assert.Equal(1, DuplicateRules.ChooseKeeper(group, KeepStrategy.Newest));
    }

    [Fact]
    public void Oldest_KeepsTheEarliestModified()
    {
        var group = Group(
            File_(@"C:\a\one.bin", new DateTime(2020, 1, 1)),
            File_(@"C:\b\two.bin", new DateTime(2024, 6, 1)));

        Assert.Equal(0, DuplicateRules.ChooseKeeper(group, KeepStrategy.Oldest));
    }

    [Fact]
    public void Shallowest_KeepsTheShortestPath()
    {
        var group = Group(
            File_(@"C:\archive\deep\deeper\one.bin"),
            File_(@"C:\one.bin"));

        Assert.Equal(1, DuplicateRules.ChooseKeeper(group, KeepStrategy.Shallowest));
    }

    /// <summary>
    /// Several copies written by one unzip share a timestamp to the tick, which is exactly the case
    /// this feature is most often pointed at. Without a tiebreak the keeper would depend on the
    /// order the rows happened to arrive in, and pressing the button twice could tick a different
    /// set — an auto-selection nobody could trust before a delete.
    /// </summary>
    [Fact]
    public void IdenticalTimestamps_TiebreakOnPath_SoTheAnswerIsStable()
    {
        var stamp = new DateTime(2024, 3, 14, 9, 26, 53);
        var forwards = Group(File_(@"C:\b\x.bin", stamp), File_(@"C:\a\x.bin", stamp));
        var backwards = Group(File_(@"C:\a\x.bin", stamp), File_(@"C:\b\x.bin", stamp));

        Assert.Equal(
            forwards.Files[DuplicateRules.ChooseKeeper(forwards, KeepStrategy.Newest)].DisplayPath,
            backwards.Files[DuplicateRules.ChooseKeeper(backwards, KeepStrategy.Newest)].DisplayPath);
    }

    // --- WastedBytes ---

    /// <summary>One copy is wanted; the rest are the waste. Counting all of them would tell the
    /// user they could reclaim space that keeping the file at all costs.</summary>
    [Fact]
    public void WastedBytes_ExcludesTheCopyBeingKept()
    {
        var group = Group(
            File_(@"C:\a\x.bin", size: 1000),
            File_(@"C:\b\x.bin", size: 1000),
            File_(@"C:\c\x.bin", size: 1000));

        Assert.Equal(2000, group.WastedBytes);
    }
}
