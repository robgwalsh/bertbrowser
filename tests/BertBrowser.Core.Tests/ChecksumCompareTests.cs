using BertBrowser.Core.Services.Duplicates;
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
}
