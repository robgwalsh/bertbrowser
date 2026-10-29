using System.Security.Cryptography;
using System.Text;
using BertBrowser.Core.Services.Checksums;
using BertBrowser.Core.Services.Duplicates;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The multi-algorithm half of <see cref="FileSystemFileHasher"/>. Its sibling,
/// <c>FileSystemFileHasherTests</c>, is deliberately unchanged: between them they hold down the
/// claim that both seams run one read loop.
/// </summary>
public sealed class FileDigesterTests : IDisposable
{
    private readonly string _root;
    private readonly FileSystemFileHasher _digester = new();

    public FileDigesterTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-digest-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
            // A file someone else still holds open is not this test's problem.
        }
    }

    private string File_(byte[] content, string name = "file.bin")
    {
        var path = Path.Combine(_root, name);
        File.WriteAllBytes(path, content);
        return path;
    }

    private static byte[] Bytes(int count)
    {
        var bytes = new byte[count];
        for (var i = 0; i < count; i++) bytes[i] = (byte)(i % 251);
        return bytes;
    }

    private FileDigests Digest(string path, params ChecksumAlgorithm[] algorithms) =>
        _digester.Digest(path, algorithms, progress: null, CancellationToken.None)
            ?? throw new InvalidOperationException("expected a digest");

    // --- the digests themselves ---

    /// <summary>
    /// CRC-32 of "123456789" is CBF43926 — the check value every CRC-32 specification publishes.
    ///
    /// This is the single most important fact in the checksum feature. System.IO.Hashing writes the
    /// checksum little-endian, so the bytes come back as 2639F4CB and have to be reversed. Get that
    /// wrong and every digest is still eight plausible hex characters, every other test still
    /// passes, and every .sfv file this app writes is silently unverifiable anywhere else.
    /// </summary>
    [Fact]
    public void Crc32MatchesTheKnownVector()
    {
        var path = File_(Encoding.ASCII.GetBytes("123456789"));

        Assert.Equal("CBF43926", Digest(path, ChecksumAlgorithm.Crc32).ByAlgorithm[ChecksumAlgorithm.Crc32]);
    }

    [Fact]
    public void EachAlgorithmAgreesWithTheBcl()
    {
        var content = Bytes(5000);
        var path = File_(content);

        var digests = Digest(path, ChecksumAlgorithm.Md5, ChecksumAlgorithm.Sha1,
            ChecksumAlgorithm.Sha256, ChecksumAlgorithm.Sha512).ByAlgorithm;

        Assert.Equal(Convert.ToHexString(MD5.HashData(content)), digests[ChecksumAlgorithm.Md5]);
        Assert.Equal(Convert.ToHexString(SHA1.HashData(content)), digests[ChecksumAlgorithm.Sha1]);
        Assert.Equal(Convert.ToHexString(SHA256.HashData(content)), digests[ChecksumAlgorithm.Sha256]);
        Assert.Equal(Convert.ToHexString(SHA512.HashData(content)), digests[ChecksumAlgorithm.Sha512]);
    }

    /// <summary>The one-pass claim: five at once must equal five separately.</summary>
    [Fact]
    public void SeveralAlgorithmsInOnePassAgreeWithOneEach()
    {
        var path = File_(Bytes(3_000_000));

        var together = Digest(path, [.. ChecksumAlgorithms.All]).ByAlgorithm;

        foreach (var algorithm in ChecksumAlgorithms.All)
        {
            Assert.Equal(Digest(path, algorithm).ByAlgorithm[algorithm], together[algorithm]);
        }
    }

    [Fact]
    public void EveryDigestIsUppercaseHexOfTheCatalogueLength()
    {
        var path = File_(Bytes(64));

        foreach (var (algorithm, digest) in Digest(path, [.. ChecksumAlgorithms.All]).ByAlgorithm)
        {
            Assert.Equal(ChecksumAlgorithms.HexLength(algorithm), digest.Length);
            Assert.Equal(digest.ToUpperInvariant(), digest);
            Assert.True(ChecksumAlgorithms.IsHex(digest));
        }
    }

    [Fact]
    public void TheAlgorithmsAskedForAreTheOnesReturned()
    {
        var path = File_(Bytes(10));

        var digests = Digest(path, ChecksumAlgorithm.Sha512, ChecksumAlgorithm.Md5).ByAlgorithm;

        Assert.Equal(2, digests.Count);
        Assert.True(digests.ContainsKey(ChecksumAlgorithm.Md5));
        Assert.True(digests.ContainsKey(ChecksumAlgorithm.Sha512));
    }

    [Fact]
    public void AskingTwiceForOneAlgorithmDigestsItOnce()
    {
        var path = File_(Bytes(10));

        Assert.Single(Digest(path, ChecksumAlgorithm.Md5, ChecksumAlgorithm.Md5).ByAlgorithm);
    }

    /// <summary>
    /// An empty request is an empty answer, not a failure. A window with every box unticked has
    /// nothing to say about a file, which is a different thing from being unable to read it.
    /// </summary>
    [Fact]
    public void AskingForNoAlgorithmsIsAnEmptyResult_NotNull()
    {
        var path = File_(Bytes(10));

        var result = _digester.Digest(path, [], progress: null, CancellationToken.None);

        Assert.NotNull(result);
        Assert.Empty(result.ByAlgorithm);
    }

    // --- progress ---

    /// <summary>
    /// Progress counts bytes off the disk, not bytes hashed. Five algorithms over one megabyte is
    /// one megabyte of progress — report it per sink and every multi-algorithm run shows a bar that
    /// runs to five hundred percent.
    /// </summary>
    [Fact]
    public void ProgressReportsEachChunkOnce_HoweverManyAlgorithms()
    {
        var content = Bytes(2_500_000);
        var path = File_(content);

        var reported = 0L;
        _digester.Digest(path, [.. ChecksumAlgorithms.All], delta => reported += delta, CancellationToken.None);

        Assert.Equal(content.Length, reported);
    }

    // --- the contract, restated because the loop is now shared ---

    [Fact]
    public void AMissingFile_IsNull_NotAThrow() =>
        Assert.Null(_digester.Digest(
            Path.Combine(_root, "nope.bin"), [ChecksumAlgorithm.Sha256], null, CancellationToken.None));

    [Fact]
    public void ADirectory_IsNull_NotAThrow() =>
        Assert.Null(_digester.Digest(_root, [ChecksumAlgorithm.Sha256], null, CancellationToken.None));

    [Fact]
    public void ACancelledToken_Throws_RatherThanReturningNull()
    {
        var path = File_(Bytes(64));
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        Assert.Throws<OperationCanceledException>(
            () => _digester.Digest(path, [ChecksumAlgorithm.Sha256], null, cts.Token));
    }

    /// <summary>
    /// The sharing rule, restated here because the read loop is now shared with the hasher: narrow
    /// the flags to <see cref="FileShare.Read"/> and this goes red alongside its sibling, which is
    /// exactly what should happen.
    /// </summary>
    [Fact]
    public void AFileSomeoneElseHasOpenForWriting_IsStillDigestible()
    {
        var path = File_(Bytes(128));

        using var holder = new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite | FileShare.Delete);

        Assert.NotNull(_digester.Digest(path, [ChecksumAlgorithm.Sha256], null, CancellationToken.None));
    }

    [Fact]
    public void AnEmptyFileDigestsToTheEmptyDigest()
    {
        var path = File_([]);

        var digests = Digest(path, ChecksumAlgorithm.Sha256, ChecksumAlgorithm.Crc32);

        Assert.Equal(0, digests.BytesRead);
        Assert.Equal(Convert.ToHexString(SHA256.HashData([])), digests.ByAlgorithm[ChecksumAlgorithm.Sha256]);
        Assert.Equal("00000000", digests.ByAlgorithm[ChecksumAlgorithm.Crc32]);
    }
}
