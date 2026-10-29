using BertBrowser.Core.Interop;
using BertBrowser.Core.Services.Changes;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChangeLogRulesTests
{
    private const uint Close = NtfsNative.UsnReasonClose;
    private const uint Create = NtfsNative.UsnReasonFileCreate;
    private const uint Delete = NtfsNative.UsnReasonFileDelete;
    private const uint RenameNew = NtfsNative.UsnReasonRenameNewName;
    private const uint DataExtend = 0x2;

    [Theory]
    [InlineData(Close | Create, false, ChangeKind.Created)]
    [InlineData(Close | Create | DataExtend, false, ChangeKind.Created)]
    [InlineData(Close | DataExtend, false, ChangeKind.Modified)]
    [InlineData(Close, false, ChangeKind.Modified)]
    [InlineData(Close | Delete, false, ChangeKind.Deleted)]
    // A temp file created and deleted inside one close: it is gone, and that is the fact.
    [InlineData(Close | Create | Delete, false, ChangeKind.Deleted)]
    [InlineData(Close | RenameNew, true, ChangeKind.Renamed)]
    // An unpaired new name is a move-in from somewhere the map could not resolve: it appeared here.
    [InlineData(Close | RenameNew, false, ChangeKind.Created)]
    [InlineData(Close | RenameNew | Delete, true, ChangeKind.Deleted)]
    public void Classify_FollowsTheIndexersPrecedence(uint reason, bool hadOldName, ChangeKind expected)
    {
        Assert.Equal(expected, ChangeLogRules.Classify(reason, hadOldName));
    }

    [Theory]
    [InlineData(@"C:\USERS\ROB\.BERTBROWSER\BERTBROWSER.DB-WAL", true)]
    [InlineData(@"C:\USERS\ROB\.BERTBROWSER\THEMES\MINE.JSON", true)]
    [InlineData(@"C:\USERS\ROB\.BERTBROWSER", true)]
    [InlineData(@"C:\USERS\ROB\.BERTBROWSERX\FILE.TXT", false)]
    [InlineData(@"C:\USERS\ROB\NOTES.TXT", false)]
    public void IsExcluded_CoversTheDataDirectoryAndNothingBeside(string pathKey, bool expected)
    {
        Assert.Equal(expected, ChangeLogRules.IsExcluded(pathKey, @"C:\USERS\ROB\.BERTBROWSER"));
    }

    [Fact]
    public void SinceUtc_PresetsSubtractFromNow()
    {
        var now = new DateTime(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);

        Assert.Equal(now.AddMinutes(-15), ChangeLogRules.SinceUtc(ChangeRange.Last15Minutes, now, null));
        Assert.Equal(now.AddHours(-1), ChangeLogRules.SinceUtc(ChangeRange.LastHour, now, null));
        Assert.Equal(now.AddHours(-6), ChangeLogRules.SinceUtc(ChangeRange.Last6Hours, now, null));
        Assert.Equal(now.AddHours(-24), ChangeLogRules.SinceUtc(ChangeRange.Last24Hours, now, null));
    }

    [Fact]
    public void SinceUtc_SinceMarkNeedsAMark()
    {
        var now = new DateTime(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);
        var mark = now.AddMinutes(-3);

        Assert.Equal(mark, ChangeLogRules.SinceUtc(ChangeRange.SinceMark, now, mark));
        Assert.Null(ChangeLogRules.SinceUtc(ChangeRange.SinceMark, now, null));
    }

    [Fact]
    public void SinceUtc_NeverReachesPastTheRetention()
    {
        // With retention at an hour, "last 24 hours" must not surface rows the writer has simply
        // not got round to pruning yet — the user was promised an hour.
        var now = new DateTime(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);
        var policy = ChangeLogPolicy.FromHours(1);

        Assert.Equal(now.AddHours(-1), ChangeLogRules.SinceUtc(ChangeRange.Last24Hours, now, null, policy));
        Assert.Equal(now.AddMinutes(-15), ChangeLogRules.SinceUtc(ChangeRange.Last15Minutes, now, null, policy));
        Assert.Equal(now.AddHours(-1), ChangeLogRules.SinceUtc(ChangeRange.SinceMark, now, now.AddHours(-5), policy));
        Assert.Null(ChangeLogRules.SinceUtc(ChangeRange.SinceMark, now, null, policy));
    }

    [Theory]
    // Recording off wins over everything: there is nothing to show however healthy the index is.
    [InlineData(false, false, true, true, false, true, ChangeTimelineAvailability.RecordingOff)]
    [InlineData(true, false, true, true, false, true, ChangeTimelineAvailability.Ready)]
    [InlineData(true, true, true, true, false, true, ChangeTimelineAvailability.Ready)]
    // The helper declined or died: nothing is being recorded.
    [InlineData(true, false, false, false, false, false, ChangeTimelineAvailability.IndexerUnavailable)]
    // Still building: recording starts when the build completes, so say that rather than "not indexed".
    [InlineData(true, false, false, false, true, true, ChangeTimelineAvailability.Building)]
    [InlineData(true, true, true, false, true, true, ChangeTimelineAvailability.Building)]
    // A scoped folder on a drive the helper does not cover (exFAT, a share).
    [InlineData(true, true, true, false, false, true, ChangeTimelineAvailability.ScopeNotIndexed)]
    // Nothing indexed and nothing building: the helper is up but found no NTFS volume.
    [InlineData(true, false, false, false, false, true, ChangeTimelineAvailability.IndexerUnavailable)]
    public void Availability_SaysWhichThingIsMissing(
        bool recordingOn, bool scoped, bool anyIndexed, bool scopeIndexed, bool isBuilding, bool indexerRunning,
        ChangeTimelineAvailability expected)
    {
        Assert.Equal(expected, ChangeLogRules.Availability(recordingOn, scoped, anyIndexed, scopeIndexed, isBuilding, indexerRunning));
    }

    [Theory]
    [InlineData(null, null)]
    [InlineData("", null)]
    [InlineData(@"C:\Work\Proj", @"C:\Work\Proj")]
    // Inside a container: the entry has no real path, so the question becomes "what changed
    // around the archive" — its folder — rather than an empty answer that reads as "nothing".
    [InlineData(@"C:\Work\a.zip", @"C:\Work")]
    [InlineData(@"C:\Work\a.zip\src\lib", @"C:\Work")]
    public void ScopeFor_LeavesContainersForTheirFolder(string? path, string? expected)
    {
        Assert.Equal(expected, ChangeLogRules.ScopeFor(path, p => p.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)));
    }

    [Fact]
    public void EmptyMessage_NamesTheRangeAndTheScope()
    {
        Assert.Equal("Nothing changed here in the last hour.", ChangeLogRules.EmptyMessage(ChangeRange.LastHour, scoped: true));
        Assert.Equal("Nothing changed on this PC in the last 15 minutes.", ChangeLogRules.EmptyMessage(ChangeRange.Last15Minutes, scoped: false));
        Assert.Equal("Nothing changed on this PC since the mark.", ChangeLogRules.EmptyMessage(ChangeRange.SinceMark, scoped: false));
    }

    [Theory]
    [InlineData(0, false, 0)]
    [InlineData(1, true, 1)]
    [InlineData(24, true, 24)]
    [InlineData(168, true, 168)]
    public void Policy_RoundTripsThroughHours(int hours, bool enabled, int backOut)
    {
        var policy = ChangeLogPolicy.FromHours(hours);

        Assert.Equal(enabled, policy.Enabled);
        Assert.Equal(backOut, policy.ToHours());
        Assert.True(ChangeLogPolicy.IsAcceptableHours(hours));
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(2)]
    [InlineData(25)]
    [InlineData(1000)]
    public void Policy_RefusesHoursOffTheMenu(int hours)
    {
        Assert.False(ChangeLogPolicy.IsAcceptableHours(hours));
        Assert.Throws<ArgumentOutOfRangeException>(() => ChangeLogPolicy.FromHours(hours));
    }

    [Fact]
    public void Policy_DefaultIsOff()
    {
        Assert.False(default(ChangeLogPolicy).Enabled);
        Assert.False(ChangeLogPolicy.Off.Enabled);
        Assert.Equal(24, ChangeLogPolicy.DefaultRetentionHours);
    }
}
