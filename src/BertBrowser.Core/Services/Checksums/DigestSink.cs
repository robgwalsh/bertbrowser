using System.IO.Hashing;
using System.Security.Cryptography;

namespace BertBrowser.Core.Services.Checksums;

/// <summary>
/// One running digest over a stream of chunks.
/// </summary>
/// <remarks>
/// Exists because .NET has two unrelated shapes for this. <see cref="IncrementalHash"/> covers the
/// four cryptographic algorithms; CRC-32 is a <c>NonCryptographicHashAlgorithm</c> with different
/// methods and a different byte order. Hiding both behind one interface is what lets the file
/// reader hold an array of digests and know nothing about which is which.
/// </remarks>
internal interface IDigestSink : IDisposable
{
    void Append(ReadOnlySpan<byte> data);

    /// <summary>The digest as uppercase hex, no separators — the app's one digest format.</summary>
    string Finish();
}

internal sealed class IncrementalDigestSink(HashAlgorithmName algorithm) : IDigestSink
{
    private readonly IncrementalHash _hash = IncrementalHash.CreateHash(algorithm);

    public void Append(ReadOnlySpan<byte> data) => _hash.AppendData(data);

    public string Finish() => Convert.ToHexString(_hash.GetHashAndReset());

    public void Dispose() => _hash.Dispose();
}

/// <summary>
/// CRC-32, the one algorithm .NET does not ship.
/// </summary>
/// <remarks>
/// <para><strong>The byte order is the whole point of this class.</strong>
/// <see cref="Crc32.GetHashAndReset()"/> writes the checksum <em>little-endian</em>, because
/// <c>NonCryptographicHashAlgorithm</c> is specified that way. Every .sfv file ever written states
/// it big-endian — CRC-32 of "123456789" is <c>CBF43926</c>, and the library hands back
/// <c>2639F4CB</c>. So the four bytes are reversed here, once, before they are ever seen.</para>
/// <para>This is the most dangerous line in the checksum feature and the reason
/// <c>ChecksumDigestTests.Crc32MatchesTheKnownVector</c> exists: getting it wrong produces a
/// plausible eight-character digest that is wrong in a way no other test would notice, and every
/// .sfv the app wrote would be quietly unverifiable anywhere else.</para>
/// </remarks>
internal sealed class Crc32DigestSink : IDigestSink
{
    private readonly Crc32 _crc = new();

    public void Append(ReadOnlySpan<byte> data) => _crc.Append(data);

    public string Finish()
    {
        var bytes = _crc.GetHashAndReset();
        Array.Reverse(bytes);
        return Convert.ToHexString(bytes);
    }

    public void Dispose()
    {
        // Nothing to release: Crc32 holds four bytes of state and no unmanaged handle.
    }
}

internal static class DigestSinks
{
    /// <summary>The one place an algorithm becomes a primitive.</summary>
    public static IDigestSink Create(ChecksumAlgorithm algorithm) => algorithm switch
    {
        ChecksumAlgorithm.Crc32 => new Crc32DigestSink(),
        ChecksumAlgorithm.Md5 => new IncrementalDigestSink(HashAlgorithmName.MD5),
        ChecksumAlgorithm.Sha1 => new IncrementalDigestSink(HashAlgorithmName.SHA1),
        ChecksumAlgorithm.Sha256 => new IncrementalDigestSink(HashAlgorithmName.SHA256),
        ChecksumAlgorithm.Sha512 => new IncrementalDigestSink(HashAlgorithmName.SHA512),
        _ => throw new ArgumentOutOfRangeException(nameof(algorithm)),
    };
}
