namespace BertBrowser.Core.Services.Checksums;

/// <summary>What digesting one file produced.</summary>
/// <param name="ByAlgorithm">A digest per algorithm asked for, uppercase hex.</param>
/// <param name="BytesRead">
/// What was actually read. Said out loud rather than assumed, for the same reason
/// <c>FileFingerprint</c> says it: a file can change size between being listed and being read.
/// </param>
public sealed record FileDigests(
    IReadOnlyDictionary<ChecksumAlgorithm, string> ByAlgorithm,
    long BytesRead);

/// <summary>
/// Digesting one file under several algorithms at once.
/// </summary>
/// <remarks>
/// <para>
/// A separate seam from <c>IFileHasher</c> on purpose, rather than an algorithm parameter added to
/// it. The duplicate finder's safety argument is that it uses SHA-256 <em>specifically</em>, because
/// what a user does with its answer is delete files; widening that interface would make the
/// argument unenforceable. Two interfaces put the rule in the type system instead — an
/// <c>IDuplicateFinder</c> cannot be handed a CRC-32 by anybody, including a future us.
/// </para>
/// <para>
/// Both are implemented by one class over one read loop, so the sharing rules, the placeholder
/// refusals and the buffer live once.
/// </para>
/// </remarks>
public interface IFileDigester
{
    /// <summary>
    /// Digests <paramref name="path"/> under every algorithm in <paramref name="algorithms"/>,
    /// reading the file exactly once.
    /// </summary>
    /// <param name="progress">
    /// Called with each chunk's byte count as it lands, never a running total, and <em>once</em> per
    /// chunk however many algorithms are running — so a caller digesting several files can add them
    /// up itself and a five-algorithm run does not report five times the bytes there are.
    /// </param>
    /// <returns>
    /// Null when the file cannot be read, or must not be. Not an error: one unreadable file marks a
    /// run incomplete and the rest carry on.
    /// </returns>
    /// <remarks>
    /// A cancelled run throws <see cref="OperationCanceledException"/> rather than returning null.
    /// The difference is load-bearing and is the same one <c>IFileHasher</c> draws: one is this file
    /// having a problem, the other is the whole run stopping.
    /// </remarks>
    FileDigests? Digest(
        string path,
        IReadOnlyList<ChecksumAlgorithm> algorithms,
        Action<long>? progress,
        CancellationToken ct);
}
