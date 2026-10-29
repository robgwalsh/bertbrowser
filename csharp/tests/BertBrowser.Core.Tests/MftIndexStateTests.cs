using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The state both <see cref="MftIndexService"/> and the out-of-process client answer from. These
/// are the semantics search routing and the status bar depend on, and they must not differ by
/// which implementation happens to be wired up.
/// </summary>
public class MftIndexStateTests
{
    [Fact]
    public void NothingIsIndexedOrBuildingInitially()
    {
        var state = new MftIndexState();

        Assert.False(state.AnyIndexed);
        Assert.False(state.IsBuilding);
        Assert.False(state.IsIndexed(@"C:\WINDOWS"));
        Assert.Equal("", state.FormatStatus());
    }

    [Fact]
    public void CompletedRootMatchesItselfAndItsDescendants()
    {
        var state = new MftIndexState();
        state.MarkComplete(@"C:\");

        Assert.True(state.AnyIndexed);
        Assert.True(state.IsIndexed(@"C:\"));
        Assert.True(state.IsIndexed(@"C:\WINDOWS"));
        Assert.True(state.IsIndexed(@"C:\WINDOWS\SYSTEM32\DRIVERS"));
    }

    [Fact]
    public void OtherVolumesAreNotCoveredByACompletedRoot()
    {
        var state = new MftIndexState();
        state.MarkComplete(@"C:\");

        Assert.False(state.IsIndexed(@"D:\"));
        Assert.False(state.IsIndexed(@"D:\DATA"));
        Assert.False(state.IsIndexed(@"\\SERVER\SHARE"));
    }

    /// <summary>
    /// The reason this goes through <see cref="Core.Paths.PathKey.IsUnder"/> rather than
    /// <c>StartsWith</c>. Mutate it to a prefix comparison and this is the theory that goes red.
    /// </summary>
    [Theory]
    [InlineData(@"C:\FOOBAR")]
    [InlineData(@"C:\FOO2")]
    [InlineData(@"C:\FOO.BAK")]
    public void ASiblingSharingThePrefixIsNotCovered(string key)
    {
        var state = new MftIndexState();
        state.MarkComplete(@"C:\FOO");

        Assert.True(state.IsIndexed(@"C:\FOO"));
        Assert.True(state.IsIndexed(@"C:\FOO\INNER"));
        Assert.False(state.IsIndexed(key));
    }

    [Fact]
    public void BuildingIsTrackedPerDriveAndCleared()
    {
        var state = new MftIndexState();

        state.MarkBuilding("C");
        Assert.True(state.IsBuilding);

        state.MarkBuilding("D");
        Assert.True(state.IsBuilding);

        state.ClearBuilding("C");
        Assert.True(state.IsBuilding);

        state.ClearBuilding("D");
        Assert.False(state.IsBuilding);
    }

    [Fact]
    public void ClearingADriveThatIsNotBuildingIsHarmless()
    {
        var state = new MftIndexState();

        state.ClearBuilding("C");

        Assert.False(state.IsBuilding);
    }

    [Fact]
    public void StatusNamesOneBuildingDrive()
    {
        var state = new MftIndexState();
        state.MarkBuilding("C");

        Assert.Equal("Indexing C:…", state.FormatStatus());
    }

    /// <summary>Ordered, so the line does not shuffle as threads finish in whatever order.</summary>
    [Fact]
    public void StatusNamesSeveralBuildingDrivesInOrder()
    {
        var state = new MftIndexState();
        state.MarkBuilding("D");
        state.MarkBuilding("C");
        state.MarkBuilding("E");

        Assert.Equal("Indexing C:, D:, E:…", state.FormatStatus());
    }

    [Fact]
    public void StatusIsEmptyOnceEveryDriveHasFinished()
    {
        var state = new MftIndexState();
        state.MarkBuilding("C");
        state.MarkComplete(@"C:\");
        state.ClearBuilding("C");

        Assert.Equal("", state.FormatStatus());
        Assert.True(state.AnyIndexed);
    }

    [Fact]
    public void CompletedRootsRoundTripForAClientResendingItsState()
    {
        var state = new MftIndexState();
        state.MarkComplete(@"C:\");
        state.MarkComplete(@"D:\");

        Assert.Equal(new[] { @"C:\", @"D:\" }, state.CompletedRoots.OrderBy(r => r, StringComparer.Ordinal));
    }

    /// <summary>What a client does when its indexer dies: forget everything, claim nothing.</summary>
    [Fact]
    public void ClearForgetsCompletionAndBuilding()
    {
        var state = new MftIndexState();
        state.MarkComplete(@"C:\");
        state.MarkBuilding("D");

        state.Clear();

        Assert.False(state.AnyIndexed);
        Assert.False(state.IsBuilding);
        Assert.False(state.IsIndexed(@"C:\WINDOWS"));
        Assert.Equal("", state.FormatStatus());
    }

    [Fact]
    public void MarkingTheSameDriveOrRootTwiceIsIdempotent()
    {
        var state = new MftIndexState();

        state.MarkBuilding("C");
        state.MarkBuilding("C");
        state.MarkComplete(@"C:\");
        state.MarkComplete(@"C:\");

        Assert.Single(state.CompletedRoots);
        state.ClearBuilding("C");
        Assert.False(state.IsBuilding);
    }
}
