using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What a plan costs in bytes, before it runs. Plans are built by hand rather than through the
/// planner: nothing here touches disk, and the point is the arithmetic and the two rules around it
/// — a rename costs nothing, and a size the index does not have is unknown rather than zero.
/// </summary>
public class TransferEstimatorTests
{
    private static TransferPlan Plan(TransferVerb verb, params PlannedTransfer[] transfers) =>
        new(verb, @"D:\dest", transfers, []);

    private static PlannedTransfer File(string source, string destination) =>
        new(source, IsDirectory: false, destination, Conflicts: false);

    private static PlannedTransfer Folder(string source, string destination) =>
        new(source, IsDirectory: true, destination, Conflicts: false);

    [Fact]
    public void AddsUpTheFilesAndFoldersItIsGiven()
    {
        var sizes = new FakeSizes();
        sizes.SetFile(@"C:\src\a.txt", 1_000);
        sizes.SetDirectory(@"C:\src\tree", 9_000, files: 4);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Copy, File(@"C:\src\a.txt", @"D:\dest\a.txt"), Folder(@"C:\src\tree", @"D:\dest\tree")),
            sizes);

        Assert.Equal(10_000, estimate.Bytes);
        Assert.Equal(5, estimate.Files);
        Assert.True(estimate.Complete);
    }

    [Fact]
    public void ASameVolumeMove_CostsNothing_BecauseItIsARename()
    {
        // The load-bearing rule. Counting these bytes puts a bar on screen that sits at zero while
        // instant renames go past, then jumps to done.
        var sizes = new FakeSizes();
        sizes.SetDirectory(@"C:\src\huge", 50L * 1024 * 1024 * 1024, files: 100_000);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Move, Folder(@"C:\src\huge", @"C:\dest\huge")), sizes);

        Assert.Equal(0, estimate.Bytes);
        Assert.Equal(0, estimate.Files);
        Assert.True(estimate.Complete);
    }

    [Fact]
    public void ACrossVolumeMove_CostsItsFullSize()
    {
        var sizes = new FakeSizes();
        sizes.SetDirectory(@"C:\src\tree", 4_096, files: 2);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Move, Folder(@"C:\src\tree", @"D:\dest\tree")), sizes);

        Assert.Equal(4_096, estimate.Bytes);
    }

    [Fact]
    public void ACopyWithinOneVolume_StillCostsItsFullSize()
    {
        var sizes = new FakeSizes();
        sizes.SetFile(@"C:\src\a.txt", 512);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Copy, File(@"C:\src\a.txt", @"C:\dest\a.txt")), sizes);

        Assert.Equal(512, estimate.Bytes);
    }

    [Fact]
    public void AFolderTheIndexHasNoRowFor_MakesTheTotalAFloor()
    {
        // A non-NTFS volume, or one still being indexed. The bytes we do know are still worth
        // having; what must not happen is presenting them as the total.
        var sizes = new FakeSizes();
        sizes.SetFile(@"C:\src\a.txt", 1_000);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Copy, File(@"C:\src\a.txt", @"D:\dest\a.txt"), Folder(@"C:\src\tree", @"D:\dest\tree")),
            sizes);

        Assert.Equal(1_000, estimate.Bytes);
        Assert.False(estimate.Complete);
        Assert.False(estimate.IsUsable);
    }

    [Fact]
    public void AnUnknownSize_IsNotTreatedAsZero()
    {
        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Copy, Folder(@"C:\src\tree", @"D:\dest\tree")), new FakeSizes());

        Assert.False(estimate.Complete);
        Assert.False(estimate.IsUsable);
    }

    [Fact]
    public void AnEmptyPlan_IsCompleteAndCostsNothing()
    {
        var estimate = TransferEstimator.Estimate(Plan(TransferVerb.Copy), new FakeSizes());

        Assert.Equal(0, estimate.Bytes);
        Assert.True(estimate.Complete);
        Assert.False(estimate.IsUsable); // nothing to draw a bar against
    }

    [Fact]
    public void APlanOfNothingButRenames_IsCompleteButNotUsable()
    {
        var sizes = new FakeSizes();
        sizes.SetFile(@"C:\src\a.txt", 1_000);

        var estimate = TransferEstimator.Estimate(
            Plan(TransferVerb.Move, File(@"C:\src\a.txt", @"C:\dest\a.txt")), sizes);

        Assert.True(estimate.Complete);
        Assert.False(estimate.IsUsable);
    }

    private sealed class FakeSizes : ITransferSizeSource
    {
        private readonly Dictionary<string, DirectorySize> _directories = new(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, long> _files = new(StringComparer.OrdinalIgnoreCase);

        internal void SetDirectory(string path, long bytes, int files) =>
            _directories[path] = new DirectorySize(bytes, files);

        internal void SetFile(string path, long bytes) => _files[path] = bytes;

        public DirectorySize? Directory(string path) =>
            _directories.TryGetValue(path, out var size) ? size : null;

        public long? File(string path) => _files.TryGetValue(path, out var size) ? size : null;
    }
}
