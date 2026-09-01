namespace BertBrowser.Core.Models;

/// <summary>
/// One indexed entry inside a subtree, addressed relative to that subtree's root.
/// </summary>
/// <param name="RelativeKey">The canonical key with the root sliced off: uppercased, no leading
/// separator. Two subtrees are paired on this string.</param>
/// <param name="Name">The leaf name in the casing the index recorded. A whole display path is
/// deliberately <em>not</em> stored per row — it is the second-largest thing a subtree scan would
/// hold and only the handful of entries that reach a dialog ever need one, so it is rebuilt from
/// the ancestors' names on demand instead.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="SizeBytes">Bytes; zero for a folder, and also zero for every row on a volume the
/// name-only build path wrote — which is why <paramref name="ModifiedUtc"/> is what says so.</param>
/// <param name="ModifiedUtc">Last write time, or <see cref="DateTime.MinValue"/> when the index
/// never recorded one.</param>
public readonly record struct FsSubtreeRow(
    string RelativeKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc);
