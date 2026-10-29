using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChecksumFileTests
{
    private const string Sha256 = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
    private const string Md5 = "D41D8CD98F00B204E9800998ECF8427E";
    private const string Crc = "CBF43926";

    // --- the *sum family: digest first ---

    [Fact]
    public void ASumLineIsDigestThenName()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  setup.exe\n", "list.sha256");

        var line = Assert.Single(parse.Lines);
        Assert.Equal("setup.exe", line.Name);
        Assert.Equal(Sha256, line.Digest);
        Assert.Empty(parse.Problems);
    }

    [Fact]
    public void TheBinaryStarIsStrippedAndRemembered()
    {
        var parse = ChecksumFile.Parse($"{Sha256} *setup.exe\n", "list.sha256");

        var line = Assert.Single(parse.Lines);
        Assert.Equal("setup.exe", line.Name);
        Assert.True(line.Binary);
    }

    [Fact]
    public void TextModeHasNoStarAndIsNotBinary()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  notes.txt\n", "list.sha256");

        Assert.False(Assert.Single(parse.Lines).Binary);
    }

    /// <summary>The digest never holds a space, so everything after the first run is the name —
    /// which is how a file called "my holiday photos.zip" survives a round trip.</summary>
    [Fact]
    public void ASumNameMayContainSpaces()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  my holiday photos.zip\n", "list.sha256");

        Assert.Equal("my holiday photos.zip", Assert.Single(parse.Lines).Name);
    }

    // --- SFV: digest last ---

    [Fact]
    public void AnSfvLineIsNameThenDigest()
    {
        var parse = ChecksumFile.Parse($"setup.exe {Crc}\n", "list.sfv");

        var line = Assert.Single(parse.Lines);
        Assert.Equal("setup.exe", line.Name);
        Assert.Equal(Crc, line.Digest);
    }

    [Fact]
    public void AnSfvNameMayContainSpaces()
    {
        var parse = ChecksumFile.Parse($"my holiday photos.zip {Crc}\n", "list.sfv");

        Assert.Equal("my holiday photos.zip", Assert.Single(parse.Lines).Name);
    }

    /// <summary>
    /// The extension decides the order, not a sniff of the content. A file whose names happen to
    /// look like digests is still an SFV if it is called one — guessing would make such a file
    /// unreadable in a way its author could never diagnose.
    /// </summary>
    [Fact]
    public void TheExtensionDecidesTheOrder_NotTheContent()
    {
        var parse = ChecksumFile.Parse($"{Md5} {Crc}\n", "list.sfv");

        var line = Assert.Single(parse.Lines);
        Assert.Equal(Md5, line.Name);
        Assert.Equal(Crc, line.Digest);
    }

    // --- an unlabelled file ---

    [Fact]
    public void AnUnlabelledFileIsSniffedAsDigestFirst()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  setup.exe\n", "CHECKSUMS.txt");

        Assert.Equal("setup.exe", Assert.Single(parse.Lines).Name);
        Assert.Equal(ChecksumAlgorithm.Sha256, parse.Algorithm);
    }

    [Fact]
    public void AnUnlabelledFileWithTrailingDigestsIsSniffedAsSfv()
    {
        var parse = ChecksumFile.Parse($"setup.exe {Crc}\n", "CHECKSUMS.txt");

        Assert.Equal("setup.exe", Assert.Single(parse.Lines).Name);
        Assert.Equal(ChecksumAlgorithm.Crc32, parse.Algorithm);
    }

    // --- comments, blanks, line endings ---

    [Fact]
    public void CommentsAndBlanksAreSkippedInEveryFormat()
    {
        var text = $"; a comment\n\n# another\n{Sha256}  setup.exe\n\n";

        var parse = ChecksumFile.Parse(text, "list.sha256");

        Assert.Single(parse.Lines);
        Assert.Empty(parse.Problems);
    }

    [Fact]
    public void CrLfIsHandled()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  a.bin\r\n{Md5}  b.bin\r\n", "list.txt");

        Assert.Equal(2, parse.Lines.Count);
    }

    // --- problems are collected, never thrown ---

    [Fact]
    public void AMalformedLineIsOneProblemAndItsNeighboursStillParse()
    {
        var text = $"{Sha256}  a.bin\nthis is not a checksum line\n{Sha256}  c.bin\n";

        var parse = ChecksumFile.Parse(text, "list.sha256");

        Assert.Equal(2, parse.Lines.Count);
        var problem = Assert.Single(parse.Problems);
        Assert.Equal(ChecksumFileProblemKind.Malformed, problem.Kind);
        Assert.Equal(2, problem.LineNumber);
    }

    [Fact]
    public void ADigestOfNoKnownLengthIsMalformed()
    {
        var parse = ChecksumFile.Parse("ABCD  a.bin\n", "list.sha256");

        Assert.Empty(parse.Lines);
        Assert.Equal(ChecksumFileProblemKind.Malformed, Assert.Single(parse.Problems).Kind);
    }

    [Fact]
    public void ADuplicateNameIsAProblem_NotTwoRows()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  a.bin\n{Md5.PadRight(64, 'A')}  a.bin\n", "list.sha256");

        Assert.Single(parse.Lines);
        Assert.Equal(ChecksumFileProblemKind.DuplicateName, Assert.Single(parse.Problems).Kind);
    }

    // --- algorithm inference ---

    [Fact]
    public void TheDigestLengthOverridesAMisleadingExtension()
    {
        // Named .md5, but every digest is 64 characters. The digests are the fact.
        var parse = ChecksumFile.Parse($"{Sha256}  a.bin\n", "list.md5");

        Assert.Equal(ChecksumAlgorithm.Sha256, parse.Algorithm);
    }

    [Fact]
    public void DigestsOfDifferentLengthsLeaveTheAlgorithmUnknown()
    {
        var parse = ChecksumFile.Parse($"{Sha256}  a.bin\n{Md5}  b.bin\n", "list.txt");

        Assert.Null(parse.Algorithm);
        Assert.Equal(2, parse.Lines.Count);
    }

    // --- rendering ---

    /// <summary>
    /// The one casing rule in the app, and the only place it is applied: sha256sum writes lower
    /// case, SFV writes upper. Internally every digest is upper, so this is purely about the file.
    /// </summary>
    [Fact]
    public void SumFilesAreLowercaseAndSfvIsUppercase()
    {
        var sum = ChecksumFile.Render(ChecksumAlgorithm.Sha256, [new ChecksumLine("a.bin", Sha256)]);
        var sfv = ChecksumFile.Render(ChecksumAlgorithm.Crc32, [new ChecksumLine("a.bin", Crc.ToLowerInvariant())]);

        Assert.Contains(Sha256.ToLowerInvariant(), sum, StringComparison.Ordinal);
        Assert.Contains(Crc, sfv, StringComparison.Ordinal);
    }

    [Fact]
    public void ASumFileRendersAsShaSumWrites() =>
        Assert.Equal(
            $"{Sha256.ToLowerInvariant()}  *setup.exe\n",
            ChecksumFile.Render(ChecksumAlgorithm.Sha256, [new ChecksumLine("setup.exe", Sha256)]));

    [Fact]
    public void AnSfvFileCarriesItsCommentHeader() =>
        Assert.StartsWith(";", ChecksumFile.Render(ChecksumAlgorithm.Crc32, [new ChecksumLine("a.bin", Crc)]));

    [Theory]
    [InlineData(ChecksumAlgorithm.Crc32, Crc)]
    [InlineData(ChecksumAlgorithm.Md5, Md5)]
    [InlineData(ChecksumAlgorithm.Sha256, Sha256)]
    public void RenderRoundTripsThroughParse(ChecksumAlgorithm algorithm, string digest)
    {
        var written = new ChecksumLine("some folder/my file.zip", digest);

        var text = ChecksumFile.Render(algorithm, [written]);
        var parse = ChecksumFile.Parse(text, "list" + ChecksumAlgorithms.FileExtension(algorithm));

        var read = Assert.Single(parse.Lines);
        Assert.Equal(written.Name, read.Name);
        Assert.Equal(digest, read.Digest, ignoreCase: true);
        Assert.Equal(algorithm, parse.Algorithm);
        Assert.Empty(parse.Problems);
    }
}
