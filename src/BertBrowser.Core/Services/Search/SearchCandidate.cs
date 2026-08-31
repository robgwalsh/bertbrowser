namespace BertBrowser.Core.Services.Search;

/// <summary>
/// One entry a query is judged against. The six fields are exactly what both consumers can
/// supply: <c>FileSystemWalker</c>'s <c>WalkEntry</c> carries every one of them, and
/// <c>FsIndexRepository</c> selects every one of them from <c>fs_entry</c>.
/// </summary>
/// <remarks>
/// <para><see cref="NameKey"/> and <see cref="PathKey"/> are <em>already uppercased</em> — the
/// walker computes the uppercased name on its way to building a path key, and the index stores
/// both folded. Matching therefore allocates nothing per candidate, which matters: this runs
/// once per entry of a live scan over a whole subtree.</para>
/// <para><strong>Never compare two of these.</strong> <see cref="Content"/> is a read cache hanging
/// off an identity rather than part of one, so the generated equality would call a candidate
/// different from itself depending on whether its file had been opened yet. Nothing compares them
/// today — there are three construction sites, all feeding a <c>Matches</c> call — and this note is
/// here so nothing starts.</para>
/// <para>A row written by <c>MftVolumeIndexer.BuildFromUsnEnum</c> carries
/// <see cref="SizeBytes"/> 0 and <see cref="ModifiedUtc"/> <see cref="DateTime.MinValue"/> —
/// that build path records names only. Terms reading those fields must treat such a row as
/// unmeasured rather than as a genuine zero.</para>
/// </remarks>
/// <param name="NameKey">The entry's name, uppercased invariantly.</param>
/// <param name="PathKey">The entry's full canonical path, uppercased invariantly.</param>
/// <param name="IsDirectory">Whether the entry is a directory.</param>
/// <param name="SizeBytes">Length in bytes; 0 for a directory and for an unmeasured row.</param>
/// <param name="ModifiedUtc">Last write time in UTC; <see cref="DateTime.MinValue"/> when unknown.</param>
/// <param name="Hidden">Effective hidden state (the entry's own, or an ancestor's).</param>
/// <param name="Content">
/// The file's decoded text, when it has been read. <strong>Null means "not read yet"</strong> —
/// which is what every first-pass producer supplies, and what makes a <c>content:</c> term answer
/// <see cref="SearchMatch.NeedsContent"/> rather than guessing. <see cref="ContentText.None"/> is
/// the different thing: read, and there was nothing to search.
/// </param>
public readonly record struct SearchCandidate(
    string NameKey,
    string PathKey,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden,
    ContentText? Content = null);
