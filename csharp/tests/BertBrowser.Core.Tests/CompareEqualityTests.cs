using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// When two files count as the same file. Every assertion here guards the same asymmetry: calling
/// two different files "the same" is what authorises a sync to delete the other side, while calling
/// two identical files different only offers a harmless copy. So every doubt resolves away from
/// <see cref="CompareVerdict.Same"/>.
/// </summary>
public sealed class CompareEqualityTests
{
    private static readonly DateTime Noon = new(2026, 3, 14, 12, 0, 0, DateTimeKind.Utc);

    private static CompareEntry File_(long size, DateTime modified, string key = "A.TXT") =>
        new(key, Path.GetFileName(key), false, size, modified);

    private static CompareEntry Folder(string key = "SUB") =>
        new(key, key, true, 0, Noon);

    // --- Timestamp tolerance ---

    [Theory]
    [InlineData(0)]
    [InlineData(1.0)]
    [InlineData(1.999)]
    public void WithinFatGranularity_IsTheSameInstant(double seconds)
    {
        var tolerance = CompareTolerance.Strict;

        Assert.Equal(0, CompareEquality.CompareTimes(Noon.AddSeconds(seconds), Noon, tolerance));
        Assert.Equal(0, CompareEquality.CompareTimes(Noon, Noon.AddSeconds(seconds), tolerance));
    }

    [Fact]
    public void PastFatGranularity_IsMeasurablyNewer()
    {
        var tolerance = CompareTolerance.Strict;

        Assert.Equal(1, CompareEquality.CompareTimes(Noon.AddSeconds(2.001), Noon, tolerance));
        Assert.Equal(-1, CompareEquality.CompareTimes(Noon, Noon.AddSeconds(2.001), tolerance));
    }

    [Theory]
    [InlineData(3600)]
    [InlineData(7200)]
    [InlineData(-3600)]
    public void AWholeHourShift_IsForgivenOnlyWhenLoose(double seconds)
    {
        var shifted = Noon.AddSeconds(seconds);

        Assert.Equal(0, CompareEquality.CompareTimes(shifted, Noon, CompareTolerance.Loose));
        Assert.NotEqual(0, CompareEquality.CompareTimes(shifted, Noon, CompareTolerance.Strict));
    }

    /// <summary>
    /// The test that stops the daylight-saving rule quietly becoming an hour of slack. A file edited
    /// fifty-five minutes ago is genuinely newer, and no filesystem rounds a timestamp by that much.
    /// Widen <c>ForgivenShifts</c> into a band and this goes red.
    /// </summary>
    [Theory]
    [InlineData(3300)]  // 55 minutes
    [InlineData(3595)]  // five seconds short of the hour
    [InlineData(3610)]  // ten seconds past it
    [InlineData(1800)]  // half an hour
    public void NearlyAWholeHour_IsNotForgivenEvenWhenLoose(double seconds)
    {
        Assert.Equal(1, CompareEquality.CompareTimes(Noon.AddSeconds(seconds), Noon, CompareTolerance.Loose));
    }

    [Theory]
    [InlineData("FAT32")]
    [InlineData("exFAT")]
    [InlineData("fat")]
    public void AFatVolumeOnEitherSide_LoosensTheTolerance(string format)
    {
        Assert.True(CompareTolerance.For(format, "NTFS").AllowWholeHourShift);
        Assert.True(CompareTolerance.For("NTFS", format).AllowWholeHourShift);
    }

    /// <summary>
    /// A volume that would not name its format — a UNC path, a mapped drive — must fail closed.
    /// An unforgiven hour shows as "newer" and offers a copy; a wrongly forgiven one offers a delete.
    /// </summary>
    [Theory]
    [InlineData(null, null)]
    [InlineData("NTFS", "NTFS")]
    [InlineData("ReFS", null)]
    public void AnUnrecognisedVolume_StaysStrict(string? left, string? right)
    {
        Assert.Equal(CompareTolerance.Strict, CompareTolerance.For(left, right));
    }

    // --- Verdicts ---

    [Fact]
    public void OneSideMissing_IsOnlyOnTheOther()
    {
        var file = File_(10, Noon);

        Assert.Equal(CompareVerdict.LeftOnly, CompareEquality.Verdict(file, null, CompareTolerance.Strict));
        Assert.Equal(CompareVerdict.RightOnly, CompareEquality.Verdict(null, file, CompareTolerance.Strict));
    }

    [Fact]
    public void SameSizeSameInstant_IsSame()
    {
        Assert.Equal(
            CompareVerdict.Same,
            CompareEquality.Verdict(File_(10, Noon), File_(10, Noon.AddSeconds(1)), CompareTolerance.Strict));
    }

    /// <summary>
    /// The size is never allowed to make two files equal on its own. A file rewritten to the same
    /// length is the everyday case, and it is why the timestamp is compared at all.
    /// </summary>
    [Fact]
    public void SameSizeDifferentInstant_IsNewer_NotSame()
    {
        Assert.Equal(
            CompareVerdict.LeftNewer,
            CompareEquality.Verdict(File_(10, Noon.AddMinutes(5)), File_(10, Noon), CompareTolerance.Strict));
    }

    [Fact]
    public void DifferentSizes_AreNeverSame_WhateverTheTimestamps()
    {
        Assert.Equal(
            CompareVerdict.Differs,
            CompareEquality.Verdict(File_(10, Noon), File_(20, Noon), CompareTolerance.Strict));

        Assert.Equal(
            CompareVerdict.RightNewer,
            CompareEquality.Verdict(File_(10, Noon), File_(20, Noon.AddHours(3)), CompareTolerance.Strict));
    }

    /// <summary>
    /// The index's name-only build path writes MinValue for every row on the volume. Trusting the
    /// sizes there would call a whole drive "the same" — and then offer to empty the other one.
    /// </summary>
    [Fact]
    public void AMissingTimestamp_IsUnknown_NotSame()
    {
        Assert.Equal(
            CompareVerdict.Unknown,
            CompareEquality.Verdict(File_(10, default), File_(10, Noon), CompareTolerance.Strict));

        Assert.Equal(
            CompareVerdict.Unknown,
            CompareEquality.Verdict(File_(10, Noon), File_(10, default), CompareTolerance.Strict));
    }

    [Fact]
    public void AFileAgainstAFolderOfTheSameName_Differs()
    {
        Assert.Equal(
            CompareVerdict.Differs,
            CompareEquality.Verdict(File_(10, Noon, "SUB"), Folder(), CompareTolerance.Strict));
    }

    /// <summary>Two folders are the neutral seed the roll-up folds children into — and also the
    /// right answer for two empty folders, which is the case that reaches this unfolded.</summary>
    [Fact]
    public void TwoFolders_AreSameBeforeTheirContentsAreFoldedIn()
    {
        Assert.Equal(
            CompareVerdict.Same,
            CompareEquality.Verdict(Folder(), Folder(), CompareTolerance.Strict));
    }
}
