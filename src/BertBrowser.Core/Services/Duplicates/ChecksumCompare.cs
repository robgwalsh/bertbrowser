namespace BertBrowser.Core.Services.Duplicates;

public enum ChecksumMatchState { Unknown, Match, Mismatch }

/// <summary>
/// Compares a computed digest against a value a user pasted in, e.g. from a download page's
/// published checksum.
/// </summary>
public static class ChecksumCompare
{
    /// <summary>
    /// Checksum listings commonly pair the hash with a file name ("&lt;hash&gt;  file.zip"), so only
    /// the first token of the input is compared; case is ignored since hex digests are
    /// conventionally lowercase but often published upper.
    /// </summary>
    public static ChecksumMatchState Evaluate(string? digest, string expectedInput)
    {
        if (digest is null) return ChecksumMatchState.Unknown;

        var expected = expectedInput.Trim();
        if (expected.Length == 0) return ChecksumMatchState.Unknown;

        var firstToken = expected.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries) is [var token, ..]
            ? token
            : expected;

        return string.Equals(firstToken, digest, StringComparison.OrdinalIgnoreCase)
            ? ChecksumMatchState.Match
            : ChecksumMatchState.Mismatch;
    }
}
