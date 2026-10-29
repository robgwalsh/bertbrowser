using System.Security.Cryptography;
using BertBrowser.Core.Services.Duplicates;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The real hasher against real files. The grouping rules are covered with a fake; what needs a
/// disk is the part that touches one — the digest being what it claims, the sample stopping where
/// it should, and above all the sharing flags, which no unit test of the scanner could catch.
/// </summary>
public sealed class FileSystemFileHasherTests : IDisposable
{
    private readonly string _root;
    private readonly FileSystemFileHasher _hasher = new();

    public FileSystemFileHasherTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-hash-{Guid.NewGuid():N}");
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
        }
    }

    private string File_(byte[] content, string name)
    {
        var path = Path.Combine(_root, name);
        File.WriteAllBytes(path, content);
        return path;
    }

    private static byte[] Bytes(int length)
    {
        var bytes = new byte[length];
        for (var i = 0; i < length; i++) bytes[i] = (byte)(i % 251);
        return bytes;
    }

    // --- the digest ---

    [Fact]
    public void KnownContent_GivesTheKnownDigest()
    {
        var content = Bytes(10_000);
        var path = File_(content, "a.bin");

        var fingerprint = _hasher.Hash(path, 0, null, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Equal(Convert.ToHexString(SHA256.HashData(content)), fingerprint.Hash);
        Assert.Equal(10_000, fingerprint.BytesRead);
    }

    /// <summary>
    /// The sampling pass depends on this exactly: read the bound, hash only that, and report how
    /// much was taken so the caller can tell "the whole file" from "the first part of it".
    /// </summary>
    [Fact]
    public void TheSampleStopsAtItsBound()
    {
        var content = Bytes(10_000);
        var path = File_(content, "a.bin");

        var fingerprint = _hasher.Hash(path, 4096, null, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Equal(Convert.ToHexString(SHA256.HashData(content.AsSpan(0, 4096))), fingerprint.Hash);
        Assert.Equal(4096, fingerprint.BytesRead);
    }

    /// <summary>A bound past the end reads to the end and says so — which is how a file smaller
    /// than the sample is recognised as having been hashed in full.</summary>
    [Fact]
    public void ABoundPastTheEnd_ReadsToTheEnd()
    {
        var content = Bytes(100);
        var path = File_(content, "a.bin");

        var fingerprint = _hasher.Hash(path, 4096, null, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Equal(100, fingerprint.BytesRead);
        Assert.Equal(Convert.ToHexString(SHA256.HashData(content)), fingerprint.Hash);
    }

    [Fact]
    public void ProgressReportsChunkDeltas_AddingUpToWhatWasRead()
    {
        var path = File_(Bytes(10_000), "a.bin");

        var total = 0L;
        var fingerprint = _hasher.Hash(path, 0, delta => total += delta, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Equal(fingerprint.BytesRead, total);
    }

    // --- the sharing rule ---

    /// <summary>
    /// <b>The rule the preview pane already follows, and the reason this class opens the way it
    /// does.</b> Hashing must never block, or be blocked by, someone else holding the file — in
    /// this app that someone is usually its own rename, move or delete executor working in the
    /// folder the user is standing in. Narrow the share flags to <c>FileShare.Read</c> and this
    /// goes red with a sharing violation.
    /// </summary>
    [Fact]
    public void AFileSomeoneElseHasOpenForWriting_IsStillHashable()
    {
        var content = Bytes(4096);
        var path = File_(content, "busy.bin");

        using var writer = new FileStream(
            path, FileMode.Open, FileAccess.Write, FileShare.ReadWrite | FileShare.Delete);

        var fingerprint = _hasher.Hash(path, 0, null, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Equal(Convert.ToHexString(SHA256.HashData(content)), fingerprint.Hash);
    }

    // --- refusals ---

    /// <summary>
    /// A file that has gone is not an error worth throwing over: one candidate's failure marks the
    /// scan incomplete and the rest carry on, which is the rule every executor in this app holds to.
    /// </summary>
    [Fact]
    public void AMissingFile_IsNull_NotAThrow()
    {
        Assert.Null(_hasher.Hash(
            Path.Combine(_root, "never-existed.bin"), 0, null, CancellationToken.None));
    }

    [Fact]
    public void ADirectory_IsNull_NotAThrow()
    {
        var directory = Path.Combine(_root, "folder");
        Directory.CreateDirectory(directory);

        Assert.Null(_hasher.Hash(directory, 0, null, CancellationToken.None));
    }

    /// <summary>
    /// Cancellation is not a per-file failure and must not come back as one: null means "this file
    /// had a problem", and conflating the two would let a cancelled run look like a disk full of
    /// unreadable files.
    /// </summary>
    [Fact]
    public void ACancelledToken_Throws_RatherThanReturningNull()
    {
        var path = File_(Bytes(4096), "a.bin");
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        Assert.Throws<OperationCanceledException>(() => _hasher.Hash(path, 0, null, cts.Token));
    }

    // --- identity ---

    /// <summary>
    /// An ordinary file carries one name, and the scanner then does no identity bookkeeping for it
    /// at all. Reporting an identity here would put every file on the shortlist through the
    /// hardlink-folding path for nothing.
    /// </summary>
    [Fact]
    public void AnOrdinaryFile_HasNoIdentity()
    {
        var path = File_(Bytes(4096), "a.bin");

        var fingerprint = _hasher.Hash(path, 0, null, CancellationToken.None);

        Assert.NotNull(fingerprint);
        Assert.Null(fingerprint.Identity);
    }
}
