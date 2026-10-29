using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ContentSettlementTests
{
    // --- the rule ---

    /// <summary>
    /// Identical bytes are stronger evidence than any timestamp, so they may raise anything to a
    /// match — including an Unknown, which is the point: an unknown that has been read is no longer
    /// unknown.
    /// </summary>
    [Theory]
    [InlineData(CompareVerdict.Differs)]
    [InlineData(CompareVerdict.LeftNewer)]
    [InlineData(CompareVerdict.RightNewer)]
    [InlineData(CompareVerdict.Unknown)]
    public void IdenticalBytesSettleToSame(CompareVerdict current) =>
        Assert.Equal(CompareVerdict.Same, ContentSettlement.Settle(current, ContentVerdict.Identical));

    /// <summary>
    /// Proving two files differ tells the comparison nothing it did not already act on, and must
    /// not <em>lower</em> a verdict either — a LeftNewer is still a LeftNewer.
    /// </summary>
    [Theory]
    [InlineData(CompareVerdict.Differs)]
    [InlineData(CompareVerdict.LeftNewer)]
    [InlineData(CompareVerdict.Same)]
    [InlineData(CompareVerdict.Unknown)]
    public void DifferingBytesChangeNothing(CompareVerdict current) =>
        Assert.Equal(current, ContentSettlement.Settle(current, ContentVerdict.Differs));

    /// <summary>
    /// The rule that matters. A comparison's "same" is what authorises a delete, so a read that
    /// failed must never produce one — not even from an Unknown, where the temptation is greatest.
    /// </summary>
    [Theory]
    [InlineData(CompareVerdict.Differs)]
    [InlineData(CompareVerdict.Unknown)]
    [InlineData(CompareVerdict.LeftOnly)]
    public void AnUnreadableSideChangesNothing(CompareVerdict current) =>
        Assert.Equal(current, ContentSettlement.Settle(current, ContentVerdict.Unreadable));

    [Fact]
    public void NothingReachesSameWithoutIdenticalBytes()
    {
        foreach (var verdict in Enum.GetValues<CompareVerdict>())
        {
            foreach (var content in new[] { ContentVerdict.Differs, ContentVerdict.Unreadable })
            {
                if (verdict is CompareVerdict.Same) continue;

                Assert.NotEqual(CompareVerdict.Same, ContentSettlement.Settle(verdict, content));
            }
        }
    }

    // --- applying it to a whole result ---

    private const string Dir = "DIR";
    private const string A = @"DIR\A.TXT";
    private const string B = @"DIR\B.TXT";
    private const string Only = @"DIR\ONLY.TXT";

    private static CompareEntry File_(string key, long size, DateTime modified) =>
        new(key, Path.GetFileName(key), IsDirectory: false, size, modified);

    private static CompareEntry Folder(string key) =>
        new(key, Path.GetFileName(key), IsDirectory: true, 0, default);

    /// <summary>A folder holding two files that differ only in size, at the same instant — which is
    /// exactly the pair a timestamp comparison has to call Differs and only bytes can settle.</summary>
    private static CompareResult Built()
    {
        var when = new DateTime(2026, 1, 1, 12, 0, 0, DateTimeKind.Utc);

        CompareEntry[] left = [Folder(Dir), File_(A, 10, when), File_(B, 10, when)];
        CompareEntry[] right = [Folder(Dir), File_(A, 20, when), File_(B, 20, when)];

        return FolderComparer.Compare(left, right, CompareTolerance.Strict);
    }

    [Fact]
    public void TheFixtureStartsOutDiffering()
    {
        var result = Built();

        Assert.Equal(CompareVerdict.Differs, result.For(A));
        Assert.Equal(CompareVerdict.Differs, result.For(B));
        Assert.Equal(CompareVerdict.Differs, result.For(Dir));
    }

    [Fact]
    public void SettlingOneLeafLeavesTheOtherAlone()
    {
        var settled = ContentSettlement.Apply(
            Built(), new Dictionary<string, ContentVerdict> { [A] = ContentVerdict.Identical });

        Assert.Equal(CompareVerdict.Same, settled.For(A));
        Assert.Equal(CompareVerdict.Differs, settled.For(B));
    }

    /// <summary>
    /// The visible point of the feature: a folder goes green once its last differing child does.
    /// CompareRules.RollUp only ever raises rank, so this only works because the fold is re-run
    /// rather than patched.
    /// </summary>
    [Fact]
    public void AFolderGoesGreenOnlyWhenEveryChildHas()
    {
        var one = ContentSettlement.Apply(
            Built(), new Dictionary<string, ContentVerdict> { [A] = ContentVerdict.Identical });

        Assert.Equal(CompareVerdict.Differs, one.For(Dir));

        var both = ContentSettlement.Apply(Built(), new Dictionary<string, ContentVerdict>
        {
            [A] = ContentVerdict.Identical,
            [B] = ContentVerdict.Identical,
        });

        Assert.Equal(CompareVerdict.Same, both.For(Dir));
    }

    [Fact]
    public void TheCountsAreRecomputed()
    {
        var before = Built();
        var after = ContentSettlement.Apply(before, new Dictionary<string, ContentVerdict>
        {
            [A] = ContentVerdict.Identical,
            [B] = ContentVerdict.Identical,
        });

        Assert.True(after.SameCount > before.SameCount);
        Assert.Equal(0, after.DifferenceCount);
        Assert.False(after.AnyDifference);
    }

    [Fact]
    public void SettlingNothingReturnsTheSameResult()
    {
        var before = Built();

        Assert.Same(before, ContentSettlement.Apply(before, new Dictionary<string, ContentVerdict>()));
    }

    /// <summary>A key nobody compared is ignored rather than invented into the result.</summary>
    [Fact]
    public void AnUnknownKeyIsIgnored()
    {
        var settled = ContentSettlement.Apply(
            Built(), new Dictionary<string, ContentVerdict> { [@"DIR\NOPE.TXT"] = ContentVerdict.Identical });

        Assert.False(settled.ByRelativeKey.ContainsKey(@"DIR\NOPE.TXT"));
        Assert.Equal(CompareVerdict.Differs, settled.For(Dir));
    }

    /// <summary>
    /// A settlement must not disturb rows it was not about — a file only one side has stays that
    /// way, whatever happened to its neighbours.
    /// </summary>
    [Fact]
    public void OneSidedRowsSurviveASettlement()
    {
        var when = new DateTime(2026, 1, 1, 12, 0, 0, DateTimeKind.Utc);
        CompareEntry[] left = [Folder(Dir), File_(A, 10, when), File_(Only, 5, when)];
        CompareEntry[] right = [Folder(Dir), File_(A, 20, when)];

        var result = FolderComparer.Compare(left, right, CompareTolerance.Strict);
        var settled = ContentSettlement.Apply(
            result, new Dictionary<string, ContentVerdict> { [A] = ContentVerdict.Identical });

        Assert.Equal(CompareVerdict.Same, settled.For(A));
        Assert.Equal(CompareVerdict.LeftOnly, settled.For(Only));

        // The folder still holds something unsettled, so it must not have gone green.
        Assert.NotEqual(CompareVerdict.Same, settled.For(Dir));
    }

    /// <summary>
    /// The sync reads its verdicts off the result, which is the whole reason settlement produces a
    /// new one rather than an overlay beside the rows. If this ever stops holding, a row saying
    /// "Same" and a sync copying it anyway is the bug that follows.
    /// </summary>
    [Fact]
    public void TheSyncSeesASettledVerdict()
    {
        var settled = ContentSettlement.Apply(
            Built(), new Dictionary<string, ContentVerdict> { [A] = ContentVerdict.Identical });

        Assert.False(CompareRules.WouldCopy(settled.For(A)));
        Assert.True(CompareRules.WouldCopy(settled.For(B)));
    }
}
