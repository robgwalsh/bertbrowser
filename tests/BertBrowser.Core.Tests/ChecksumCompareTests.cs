using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChecksumCompareTests
{
    private const string Digest = "7B0E07BCF7285BAE7B39CFF9BEF5E05EFE2BC1E5EF221130E0A8F338EAA4E849";

    [Fact]
    public void NoDigestYetIsUnknown() =>
        Assert.Equal(ChecksumMatchState.Unknown, ChecksumCompare.Evaluate(null, "anything"));

    [Fact]
    public void BlankInputIsUnknown() =>
        Assert.Equal(ChecksumMatchState.Unknown, ChecksumCompare.Evaluate(Digest, "   "));

    [Fact]
    public void AnExactCaseInsensitiveMatchMatches() =>
        Assert.Equal(ChecksumMatchState.Match, ChecksumCompare.Evaluate(Digest, Digest.ToLowerInvariant()));

    [Fact]
    public void ADifferentValueMismatches() =>
        Assert.Equal(ChecksumMatchState.Mismatch, ChecksumCompare.Evaluate(Digest, "not the right hash"));

    [Fact]
    public void OnlyTheFirstTokenIsCompared() =>
        // sha256sum-style listings pair the hash with a file name.
        Assert.Equal(ChecksumMatchState.Match, ChecksumCompare.Evaluate(Digest, $"{Digest}  download.zip"));

    [Fact]
    public void SurroundingWhitespaceIsIgnored() =>
        Assert.Equal(ChecksumMatchState.Match, ChecksumCompare.Evaluate(Digest, $"  {Digest}\n"));

    /// <summary>
    /// An SFV line puts the name first and the checksum last — the opposite of every other format.
    /// Taking the first token unconditionally read the file name as the digest and reported a
    /// mismatch against a checksum that was in fact correct.
    /// </summary>
    [Fact]
    public void AnSfvStyleLineIsRecognisedFromTheOtherEnd() =>
        Assert.Equal(ChecksumMatchState.Match, ChecksumCompare.Evaluate("CBF43926", "download.zip CBF43926"));

    /// <summary>
    /// What decides which end holds the digest is its shape, so a name that happens to be hex but
    /// is not a digest length cannot be mistaken for one.
    /// </summary>
    [Fact]
    public void ATokenThatIsNotADigestLengthIsNotTakenAsOne() =>
        Assert.Null(ChecksumCompare.ExtractDigest("deadbeef0 notes.txt"));

    [Fact]
    public void TheFirstTokenWinsWhenBothEndsLookLikeDigests() =>
        Assert.Equal(Digest, ChecksumCompare.ExtractDigest($"{Digest} CBF43926"));
}
