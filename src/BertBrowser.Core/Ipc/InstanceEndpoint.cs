namespace BertBrowser.Core.Ipc;

/// <summary>
/// The name of the pipe a running copy of BertBrowser listens on so a second launch can hand over
/// its command line instead of starting a whole second app.
/// </summary>
/// <remarks>
/// <para>
/// <b>The name is random, and that is the security-relevant part of it.</b> Pipe names live in one
/// machine-wide namespace with no per-user partitioning: any account on the machine may create any
/// name that is not already taken. A predictable <c>BertBrowser.&lt;SID&gt;</c> could therefore be
/// claimed by another signed-in user before the real copy started, and the consequences were not
/// subtle — the genuine first instance could never create its listener, and every launch after that
/// would connect to the squatter, write the folder path it was asked to open, and exit having opened
/// nothing. A DACL cannot answer this: it governs who may <em>open</em> an endpoint, not who may take
/// the name. Randomness can, so the name carries a 128-bit nonce exactly as the index pipe's does,
/// and the copy that owns it publishes the name where only its own account can read it.
/// </para>
/// <para>
/// Kept here, beside <see cref="IndexerArguments.IsAcceptablePipeName"/> and
/// <see cref="PipeIdentity"/>, for the same reason those are: the rule is worth a test, and a rule
/// that lives in the WPF project is a rule nothing asserts.
/// </para>
/// </remarks>
public static class InstanceEndpoint
{
    /// <summary>16 random bytes, hex-encoded.</summary>
    public const int NonceLength = 32;

    /// <summary>Long enough for the longest SID plus the nonce, short enough not to be a payload.</summary>
    public const int MaxNameLength = 256;

    /// <summary>What every endpoint for <paramref name="userKey"/> begins with.</summary>
    public static string Prefix(string userKey) => $"BertBrowser.{userKey}.";

    /// <summary>The endpoint name for one user and one nonce.</summary>
    public static string Name(string userKey, string nonce) => Prefix(userKey) + nonce;

    /// <summary>
    /// Whether <paramref name="candidate"/> is a name this app would have produced for
    /// <paramref name="userKey"/>.
    /// </summary>
    /// <remarks>
    /// This is <b>not</b> the gate — the published file sits where only one account can write, and
    /// that is what keeps a name trustworthy. What this stops is a truncated, hand-edited or
    /// half-written file turning into a pipe path: the name is about to be interpolated into
    /// <c>\\.\pipe\</c>, so a separator in it would name a different object entirely, and a name with
    /// no nonce left on it would be exactly the guessable one this design exists to avoid.
    /// </remarks>
    public static bool IsAcceptable(string? candidate, string? userKey)
    {
        if (string.IsNullOrEmpty(candidate) || string.IsNullOrEmpty(userKey)) return false;
        if (candidate.Length > MaxNameLength) return false;

        var prefix = Prefix(userKey);
        if (!candidate.StartsWith(prefix, StringComparison.Ordinal)) return false;

        // Everything after the prefix is the nonce, and nothing else — which rules out separators,
        // wildcards and control characters without having to name them.
        var nonce = candidate.AsSpan(prefix.Length);
        if (nonce.Length != NonceLength) return false;

        foreach (var c in nonce)
        {
            if (!char.IsAsciiHexDigitUpper(c)) return false;
        }
        return true;
    }
}
