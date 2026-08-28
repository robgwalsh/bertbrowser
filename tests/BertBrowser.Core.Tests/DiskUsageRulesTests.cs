using BertBrowser.Core.Models;
using BertBrowser.Core.Services.DiskUsage;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class DiskUsageRulesTests
{
    private static DiskUsageNode Node(string name, long? size, bool isDir = true) =>
        new($@"C:\ROOT\{name.ToUpperInvariant()}", $@"C:\Root\{name}", name, isDir, size, false, false);

    // --- Classify: the largest-files verdict ---

    /// <summary>
    /// The one that matters. The FSCTL_ENUM_USN_DATA build path writes every row with
    /// size_bytes = 0 and fills no dir_size_cache, so a top-N by size comes back full of rows that
    /// are all zero. That is a volume nobody measured, not a disk full of empty files — and since
    /// the query orders by size descending, a zero in the <em>largest</em> slot proves it.
    /// Make Classify return Ready here and this goes red; that mutation is exactly the bug that
    /// puts a screenful of "0 B" in front of the user.
    /// </summary>
    [Fact]
    public void AllZeroSizes_IsNotSizeData_NotAnEmptyDisk()
    {
        var verdict = DiskUsageRules.Classify(
            @"C:\", rowCount: 500, largestSizeBytes: 0, isBuilding: false, isIndexed: true);

        Assert.Equal(DiskUsageAvailability.NoSizeData, verdict);
    }

    /// <summary>The sizeless shape is decided before the building check, or a volume that is both
    /// mid-build and sizeless would be reported as merely incomplete and drawn as zeros.</summary>
    [Fact]
    public void AllZeroSizes_BeatsBuilding()
    {
        Assert.Equal(
            DiskUsageAvailability.NoSizeData,
            DiskUsageRules.Classify(@"C:\", 500, 0, isBuilding: true, isIndexed: false));
    }

    [Fact]
    public void RowsWithRealSizes_AreReady()
    {
        Assert.Equal(
            DiskUsageAvailability.Ready,
            DiskUsageRules.Classify(@"C:\", 500, 4_096, isBuilding: false, isIndexed: true));
    }

    /// <summary>Partial results while a volume is still being read are a floor, and have to say so
    /// rather than presenting themselves as the answer.</summary>
    [Fact]
    public void RowsWhileStillBuilding_AreBuilding()
    {
        Assert.Equal(
            DiskUsageAvailability.Building,
            DiskUsageRules.Classify(@"C:\", 12, 4_096, isBuilding: true, isIndexed: false));
    }

    [Fact]
    public void NoRows_WhileBuilding_IsBuildingNotEmpty()
    {
        Assert.Equal(
            DiskUsageAvailability.Building,
            DiskUsageRules.Classify(@"C:\", 0, 0, isBuilding: true, isIndexed: false));
    }

    [Fact]
    public void NoRows_NotIndexed_SaysSoSoTheViewCanOfferARetry()
    {
        Assert.Equal(
            DiskUsageAvailability.NotIndexed,
            DiskUsageRules.Classify(@"C:\", 0, 0, isBuilding: false, isIndexed: false));
    }

    /// <summary>An indexed volume that genuinely holds no files is the one case where "nothing"
    /// is the true answer.</summary>
    [Fact]
    public void NoRows_ButIndexed_IsAGenuinelyEmptyResult()
    {
        Assert.Equal(
            DiskUsageAvailability.Ready,
            DiskUsageRules.Classify(@"C:\", 0, 0, isBuilding: false, isIndexed: true));
    }

    /// <summary>Null is the deliberate whole-PC scope; only an empty string is a bad root.</summary>
    [Fact]
    public void NullRootIsWholePc_EmptyRootIsNotAPath()
    {
        Assert.Equal(
            DiskUsageAvailability.Ready,
            DiskUsageRules.Classify(null, 5, 100, isBuilding: false, isIndexed: true));
        Assert.Equal(
            DiskUsageAvailability.NotAPath,
            DiskUsageRules.Classify("", 5, 100, isBuilding: false, isIndexed: true));
    }

    // --- ClassifyBreakdown: one folder's verdict, which weighs different evidence ---

    /// <summary>
    /// A folder of empty files really is all zeros, and saying "no size data" about it would be a
    /// lie about ordinary content. Point Classify's all-zero rule at a breakdown and this goes red.
    /// </summary>
    [Fact]
    public void AFolderOfEmptyFiles_IsReady_NotNoSizeData()
    {
        Assert.Equal(
            DiskUsageAvailability.Ready,
            DiskUsageRules.ClassifyBreakdown(
                directoryChildCount: 0, measuredDirectoryCount: 0, isBuilding: false, isIndexed: false));
    }

    [Fact]
    public void MeasuredSubfoldersAreReady()
    {
        Assert.Equal(
            DiskUsageAvailability.Ready,
            DiskUsageRules.ClassifyBreakdown(3, 3, isBuilding: false, isIndexed: true));
    }

    [Fact]
    public void SubfoldersNoneMeasured_WhileBuilding_IsBuilding()
    {
        Assert.Equal(
            DiskUsageAvailability.Building,
            DiskUsageRules.ClassifyBreakdown(3, 0, isBuilding: true, isIndexed: false));
    }

    /// <summary>Indexed, has subfolders, and not one total anywhere: the sizeless build.</summary>
    [Fact]
    public void SubfoldersNoneMeasured_ButIndexed_IsNoSizeData()
    {
        Assert.Equal(
            DiskUsageAvailability.NoSizeData,
            DiskUsageRules.ClassifyBreakdown(3, 0, isBuilding: false, isIndexed: true));
    }

    [Fact]
    public void SubfoldersNoneMeasured_AndNotIndexed_IsNotIndexed()
    {
        Assert.Equal(
            DiskUsageAvailability.NotIndexed,
            DiskUsageRules.ClassifyBreakdown(3, 0, isBuilding: false, isIndexed: false));
    }

    // --- Unaccounted: the arithmetic form of "never zero" ---

    [Fact]
    public void Unaccounted_IsTheRemainderWhenEverythingIsKnown()
    {
        var remainder = DiskUsageRules.Unaccounted(1_000, [Node("a", 600), Node("b", 300)]);

        Assert.Equal(100, remainder);
    }

    /// <summary>
    /// Treating an unknown child as zero would silently attribute its bytes to the parent's own
    /// loose files — a wrong number, not a smaller one. Change the sum to <c>?? 0</c> and this
    /// goes red.
    /// </summary>
    [Fact]
    public void Unaccounted_IsNullWhenAnyChildIsUnknown()
    {
        Assert.Null(DiskUsageRules.Unaccounted(1_000, [Node("a", 600), Node("b", null)]));
    }

    [Fact]
    public void Unaccounted_IsNullWhenTheParentTotalIsUnknown()
    {
        Assert.Null(DiskUsageRules.Unaccounted(null, [Node("a", 600)]));
    }

    /// <summary>A dir_size_cache row that predates its children's makes the remainder negative.
    /// There is no negative space, so the honest answer is "cannot say" rather than a bar drawn
    /// backwards.</summary>
    [Fact]
    public void Unaccounted_IsNullRatherThanNegativeWhenTheParentRowIsStale()
    {
        Assert.Null(DiskUsageRules.Unaccounted(500, [Node("a", 600), Node("b", 300)]));
    }

    [Fact]
    public void Unaccounted_IsTheWholeTotalWhenThereAreNoChildren()
    {
        Assert.Equal(1_000, DiskUsageRules.Unaccounted(1_000, []));
    }
}
