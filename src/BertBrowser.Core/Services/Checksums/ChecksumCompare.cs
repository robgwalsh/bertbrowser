namespace BertBrowser.Core.Services.Checksums;

public enum ChecksumMatchState { Unknown, Match, Mismatch }

/// <summary>
/// Compares a computed digest against a value a user pasted in, e.g. from a download page's
/// published checksum.
/// </summary>
public static class ChecksumCompare
{
    /// <summary>
    /// The digest inside whatever the user pasted, or null if there isn't one that looks like a
    /// digest.
    /// </summary>
    /// <remarks>
    /// People paste a whole line, not a bare hash, and the two conventions put the hash at opposite
    /// ends: <c>sha256sum</c> writes <c>&lt;hash&gt;  file.zip</c> and an SFV file writes
    /// <c>file.zip A1B2C3D4</c>. So both ends are considered, and what settles it is the shape —
    /// hex, of a length exactly one algorithm produces. A file name is almost never both.
    ///
    /// Preferring the first token when both ends qualify keeps the common case exact: a file
    /// genuinely named after its own hash is a curiosity, and it is the <c>sha256sum</c> form that
    /// is pasted a thousand times more often.
    /// </remarks>
    public static string? ExtractDigest(string pasted)
    {
        var tokens = pasted.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (tokens.Length == 0) return null;

        if (ChecksumAlgorithms.Recognise(tokens[0]) is not null) return tokens[0];

        var last = tokens[^1];
        return ChecksumAlgorithms.Recognise(last) is not null ? last : null;
    }

    /// <summary>
    /// Whether a digest matches what the user pasted.
    /// </summary>
    /// <remarks>
    /// Falls back to the first token when nothing in the paste looks like a known digest, so a
    /// truncated or mistyped hash still compares — and mismatches — rather than silently reading as
    /// "nothing to compare". Case is ignored: hex digests are conventionally lower case and are
    /// very often published upper.
    /// </remarks>
    public static ChecksumMatchState Evaluate(string? digest, string expectedInput)
    {
        if (digest is null) return ChecksumMatchState.Unknown;

        var expected = expectedInput.Trim();
        if (expected.Length == 0) return ChecksumMatchState.Unknown;

        var candidate = ExtractDigest(expected)
            ?? expected.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries)[0];

        return string.Equals(candidate, digest, StringComparison.OrdinalIgnoreCase)
            ? ChecksumMatchState.Match
            : ChecksumMatchState.Mismatch;
    }
}
