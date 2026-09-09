using System.Security.Cryptography;
using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChecksumAlgorithmsTests
{
    [Fact]
    public void EveryAlgorithmIsInTheCatalogue() =>
        Assert.Equal(Enum.GetValues<ChecksumAlgorithm>().Length, ChecksumAlgorithms.All.Count);

    /// <summary>
    /// The lengths are computed from the primitives rather than restated, so the table cannot drift
    /// from what the algorithms actually produce.
    /// </summary>
    [Theory]
    [InlineData(ChecksumAlgorithm.Md5, 16)]
    [InlineData(ChecksumAlgorithm.Sha1, 20)]
    [InlineData(ChecksumAlgorithm.Sha256, 32)]
    [InlineData(ChecksumAlgorithm.Sha512, 64)]
    [InlineData(ChecksumAlgorithm.Crc32, 4)]
    public void HexLengthIsTwiceTheDigestSize(ChecksumAlgorithm algorithm, int digestBytes) =>
        Assert.Equal(digestBytes * 2, ChecksumAlgorithms.HexLength(algorithm));

    [Fact]
    public void TheCryptographicLengthsAgreeWithTheBcl()
    {
        Assert.Equal(MD5.HashSizeInBytes * 2, ChecksumAlgorithms.HexLength(ChecksumAlgorithm.Md5));
        Assert.Equal(SHA1.HashSizeInBytes * 2, ChecksumAlgorithms.HexLength(ChecksumAlgorithm.Sha1));
        Assert.Equal(SHA256.HashSizeInBytes * 2, ChecksumAlgorithms.HexLength(ChecksumAlgorithm.Sha256));
        Assert.Equal(SHA512.HashSizeInBytes * 2, ChecksumAlgorithms.HexLength(ChecksumAlgorithm.Sha512));
    }

    /// <summary>
    /// No two algorithms share a digest length. This is what makes <see cref="ChecksumAlgorithms.
    /// FromDigestLength"/> possible at all, and so what lets the compare box work out for itself
    /// which algorithm someone pasted.
    /// </summary>
    [Fact]
    public void NoTwoAlgorithmsShareADigestLength() =>
        Assert.Equal(
            ChecksumAlgorithms.All.Count,
            ChecksumAlgorithms.All.Select(ChecksumAlgorithms.HexLength).Distinct().Count());

    [Fact]
    public void NoTwoAlgorithmsShareAnExtension() =>
        Assert.Equal(
            ChecksumAlgorithms.All.Count,
            ChecksumAlgorithms.All.Select(ChecksumAlgorithms.FileExtension)
                .Distinct(StringComparer.OrdinalIgnoreCase).Count());

    [Fact]
    public void EveryExtensionRoundTrips()
    {
        foreach (var algorithm in ChecksumAlgorithms.All)
        {
            Assert.Equal(algorithm, ChecksumAlgorithms.FromExtension(ChecksumAlgorithms.FileExtension(algorithm)));
            Assert.Equal(algorithm, ChecksumAlgorithms.FromExtension("list" + ChecksumAlgorithms.FileExtension(algorithm)));
        }
    }

    [Fact]
    public void EveryDigestLengthRoundTrips()
    {
        foreach (var algorithm in ChecksumAlgorithms.All)
        {
            Assert.Equal(algorithm, ChecksumAlgorithms.FromDigestLength(ChecksumAlgorithms.HexLength(algorithm)));
        }
    }

    [Fact]
    public void AnUnknownExtensionIsNull()
    {
        Assert.Null(ChecksumAlgorithms.FromExtension("notes.txt"));
        Assert.Null(ChecksumAlgorithms.FromExtension(""));
        Assert.Null(ChecksumAlgorithms.FromExtension(null));
    }

    [Fact]
    public void AnUnknownDigestLengthIsNull()
    {
        Assert.Null(ChecksumAlgorithms.FromDigestLength(63));
        Assert.Null(ChecksumAlgorithms.FromDigestLength(0));
    }

    [Fact]
    public void RecogniseNeedsBothHexAndAKnownLength()
    {
        Assert.Equal(ChecksumAlgorithm.Crc32, ChecksumAlgorithms.Recognise("CBF43926"));
        Assert.Equal(ChecksumAlgorithm.Crc32, ChecksumAlgorithms.Recognise("cbf43926"));

        Assert.Null(ChecksumAlgorithms.Recognise("CBF4392Z"));  // not hex
        Assert.Null(ChecksumAlgorithms.Recognise("CBF4392"));   // not a known length
        Assert.Null(ChecksumAlgorithms.Recognise(""));
    }

    /// <summary>
    /// Normalising here rather than at each call site is what stops two windows disagreeing about
    /// the order they show digests in.
    /// </summary>
    [Fact]
    public void NormaliseDeduplicatesAndOrders()
    {
        var normalised = ChecksumAlgorithms.Normalise(
            [ChecksumAlgorithm.Sha512, ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha512]);

        Assert.Equal([ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha512], normalised);
    }

    [Fact]
    public void NormaliseOfNothingIsNothing() => Assert.Empty(ChecksumAlgorithms.Normalise([]));

    /// <summary>Only SFV is written upper case; the *sum family is what people diff against.</summary>
    [Fact]
    public void OnlySfvIsWrittenUppercase()
    {
        Assert.False(ChecksumAlgorithms.IsWrittenLowercase(ChecksumAlgorithm.Crc32));

        foreach (var algorithm in ChecksumAlgorithms.All.Where(a => a is not ChecksumAlgorithm.Crc32))
        {
            Assert.True(ChecksumAlgorithms.IsWrittenLowercase(algorithm));
        }
    }
}
