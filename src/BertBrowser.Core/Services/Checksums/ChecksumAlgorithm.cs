namespace BertBrowser.Core.Services.Checksums;

/// <summary>
/// The hash algorithms the checksum tool offers, weakest first.
/// </summary>
/// <remarks>
/// The union of what Directory Opus, Total Commander, XYplorer, Double Commander and Multi
/// Commander publish, which is what makes a checksum from any of them verifiable here. Ordered
/// deliberately: <see cref="Crc32"/> catches corruption and nothing else, and sits at the end of a
/// list rather than the front of one.
/// </remarks>
public enum ChecksumAlgorithm
{
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Crc32,
}

/// <summary>
/// What each algorithm is called, how long its digest is, and which checksum-file extension
/// belongs to it. One table, so a label, a file name and a parser cannot disagree.
/// </summary>
public static class ChecksumAlgorithms
{
    /// <summary>Every algorithm, in the order a UI should offer them.</summary>
    public static readonly IReadOnlyList<ChecksumAlgorithm> All =
    [
        ChecksumAlgorithm.Md5,
        ChecksumAlgorithm.Sha1,
        ChecksumAlgorithm.Sha256,
        ChecksumAlgorithm.Sha512,
        ChecksumAlgorithm.Crc32,
    ];

    /// <summary>What a person calls it.</summary>
    public static string DisplayName(ChecksumAlgorithm algorithm) => algorithm switch
    {
        ChecksumAlgorithm.Crc32 => "CRC-32",
        ChecksumAlgorithm.Md5 => "MD5",
        ChecksumAlgorithm.Sha1 => "SHA-1",
        ChecksumAlgorithm.Sha256 => "SHA-256",
        ChecksumAlgorithm.Sha512 => "SHA-512",
        _ => throw new ArgumentOutOfRangeException(nameof(algorithm)),
    };

    /// <summary>
    /// Characters in the hex digest. Distinct across all five, which is the whole reason
    /// <see cref="FromDigestLength"/> can work.
    /// </summary>
    public static int HexLength(ChecksumAlgorithm algorithm) => algorithm switch
    {
        ChecksumAlgorithm.Crc32 => 8,
        ChecksumAlgorithm.Md5 => 32,
        ChecksumAlgorithm.Sha1 => 40,
        ChecksumAlgorithm.Sha256 => 64,
        ChecksumAlgorithm.Sha512 => 128,
        _ => throw new ArgumentOutOfRangeException(nameof(algorithm)),
    };

    /// <summary>The conventional extension of a checksum file holding this algorithm.</summary>
    public static string FileExtension(ChecksumAlgorithm algorithm) => algorithm switch
    {
        ChecksumAlgorithm.Crc32 => ".sfv",
        ChecksumAlgorithm.Md5 => ".md5",
        ChecksumAlgorithm.Sha1 => ".sha1",
        ChecksumAlgorithm.Sha256 => ".sha256",
        ChecksumAlgorithm.Sha512 => ".sha512",
        _ => throw new ArgumentOutOfRangeException(nameof(algorithm)),
    };

    /// <summary>
    /// Whether a checksum file of this algorithm is conventionally written in lower case.
    /// </summary>
    /// <remarks>
    /// The <em>only</em> place casing is decided. Digests are uppercase everywhere inside this app
    /// — <c>Convert.ToHexString</c> produces them that way and the duplicate finder groups on them
    /// — and are lowered solely on the way out to a file, because that is what <c>sha256sum</c>
    /// writes and what a person diffing two checksum files expects. SFV is the exception: its
    /// convention is upper case. Reading never consults this, since every comparison in the app is
    /// <see cref="StringComparison.OrdinalIgnoreCase"/>.
    /// </remarks>
    public static bool IsWrittenLowercase(ChecksumAlgorithm algorithm) =>
        algorithm is not ChecksumAlgorithm.Crc32;

    /// <summary>
    /// The algorithm a checksum file's name implies, or null when the name says nothing.
    /// </summary>
    /// <param name="fileNameOrExtension">A file name or a bare extension; either works.</param>
    public static ChecksumAlgorithm? FromExtension(string? fileNameOrExtension)
    {
        if (fileNameOrExtension is not { Length: > 0 }) return null;

        var extension = fileNameOrExtension.StartsWith('.') && !fileNameOrExtension.Contains('\\')
            ? fileNameOrExtension
            : Path.GetExtension(fileNameOrExtension);

        foreach (var algorithm in All)
        {
            if (string.Equals(extension, FileExtension(algorithm), StringComparison.OrdinalIgnoreCase))
                return algorithm;
        }

        return null;
    }

    /// <summary>
    /// The algorithm a digest of this many hex characters must have come from, or null.
    /// </summary>
    /// <remarks>
    /// This is what lets the compare box work out for itself what someone pasted: the five lengths
    /// do not collide, so a 32-character paste is an MD5 and nothing else. Length alone is not
    /// proof the text <em>is</em> a digest — the caller checks it is hex — only of which algorithm
    /// it would belong to.
    /// </remarks>
    public static ChecksumAlgorithm? FromDigestLength(int hexLength)
    {
        foreach (var algorithm in All)
        {
            if (HexLength(algorithm) == hexLength) return algorithm;
        }

        return null;
    }

    /// <summary>Whether every character is a hex digit, and there is at least one.</summary>
    public static bool IsHex(ReadOnlySpan<char> text)
    {
        if (text.Length == 0) return false;

        foreach (var c in text)
        {
            if (!char.IsAsciiHexDigit(c)) return false;
        }

        return true;
    }

    /// <summary>
    /// The algorithm a token looks like it belongs to: hex, of a length only one algorithm uses.
    /// </summary>
    public static ChecksumAlgorithm? Recognise(ReadOnlySpan<char> token) =>
        IsHex(token) ? FromDigestLength(token.Length) : null;

    /// <summary>
    /// A requested set reduced to each algorithm once, in <see cref="All"/> order.
    /// </summary>
    /// <remarks>
    /// Callers pass a list because a set has no order and the results are rendered in one. Doing
    /// the reduction here rather than at each call site is what stops two windows disagreeing about
    /// the order they show digests in.
    /// </remarks>
    public static IReadOnlyList<ChecksumAlgorithm> Normalise(IEnumerable<ChecksumAlgorithm> requested)
    {
        var wanted = new HashSet<ChecksumAlgorithm>(requested);
        var ordered = new List<ChecksumAlgorithm>(wanted.Count);

        foreach (var algorithm in All)
        {
            if (wanted.Contains(algorithm)) ordered.Add(algorithm);
        }

        return ordered;
    }
}
