namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// One entry on one side of a comparison, addressed relative to that side's root.
/// </summary>
/// <param name="RelativeKey">Uppercased, '\'-separated, with no leading separator — a
/// <see cref="Paths.PathKey"/> with the root sliced off. Pairing the two sides by this string is
/// what makes the comparison agree with the rest of the app about what "the same path" means.</param>
/// <param name="Name">The leaf name, display-cased. There is deliberately no whole display path
/// here: over a real 1.65-million-row index that second copy of every path is the largest
/// avoidable thing a comparison holds, and only the entries that reach a dialog need one — so
/// <see cref="CompareResult.DisplayPath"/> rebuilds them from the ancestors' names instead.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="SizeBytes">Bytes for a file; ignored for a folder, whose size is never compared.</param>
/// <param name="ModifiedUtc">Last write time. <see cref="DateTime.MinValue"/> means <em>unknown</em>
/// — the index's name-only build path writes exactly that — and forces
/// <see cref="CompareVerdict.Unknown"/> rather than a guess.</param>
public readonly record struct CompareEntry(
    string RelativeKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc);

/// <summary>
/// How the two sides of one relative path relate. Side-neutral: it names the relationship, not
/// what either list should look like — <see cref="CompareRules.RowState"/> projects it per side.
/// </summary>
public enum CompareVerdict
{
    /// <summary>Could not be established, because a side reported no timestamp. Never rendered as
    /// a match and never turned into a sync action: this is the one verdict that must never be
    /// produced by guessing, because <see cref="Same"/> is what authorises a delete.</summary>
    Unknown,

    /// <summary>Both sides exist and match.</summary>
    Same,

    /// <summary>Present on the left, absent on the right.</summary>
    LeftOnly,

    /// <summary>Present on the right, absent on the left.</summary>
    RightOnly,

    /// <summary>Both exist; the left is measurably newer.</summary>
    LeftNewer,

    /// <summary>Both exist; the right is measurably newer.</summary>
    RightNewer,

    /// <summary>Both exist and are not the same, but neither is measurably newer — equal
    /// timestamps with unequal sizes, or a file on one side and a folder on the other.</summary>
    Differs,
}

/// <summary>Which side of a comparison a listing belongs to.</summary>
public enum CompareSide
{
    Left,
    Right,
}

/// <summary>
/// What one row shows: the per-side projection of a <see cref="CompareVerdict"/>. Separate from the
/// verdict because "only here" and "older" mean opposite things on the two sides, and a row can
/// only ever say something about itself.
/// </summary>
public enum CompareRowState
{
    /// <summary>Not part of a comparison, or nothing to say on this side.</summary>
    None,
    Unknown,
    Same,
    OnlyHere,
    Newer,
    Older,
    Differs,
}

/// <summary>Where a side's listing came from. Not shown to the user — a compare always works, so
/// "the index cannot answer this" selects a source rather than producing a message.</summary>
public enum CompareSourceKind
{
    /// <summary>A range scan of <c>fs_entry</c>.</summary>
    Index,

    /// <summary>A live walk of the directory tree.</summary>
    Walk,
}

/// <summary>Slicing relative keys apart. Ordinal throughout: relative keys are already uppercased.</summary>
public static class CompareKeys
{
    public const char Separator = '\\';

    /// <summary>Every ancestor key of <paramref name="relativeKey"/>, nearest first, excluding the
    /// key itself and excluding the root (which is the empty string and has no row).</summary>
    public static IEnumerable<string> Ancestors(string relativeKey)
    {
        ArgumentNullException.ThrowIfNull(relativeKey);

        var cut = relativeKey.LastIndexOf(Separator);
        while (cut > 0)
        {
            var ancestor = relativeKey[..cut];
            yield return ancestor;
            cut = ancestor.LastIndexOf(Separator);
        }
    }

    /// <summary>True when <paramref name="key"/> is at or below <paramref name="folderKey"/>.
    /// The separator check is what stops "SRC2\A" reading as being inside "SRC".</summary>
    public static bool IsAtOrUnder(string key, string folderKey)
    {
        if (folderKey.Length == 0) return true;
        if (!key.StartsWith(folderKey, StringComparison.Ordinal)) return false;
        return key.Length == folderKey.Length || key[folderKey.Length] == Separator;
    }
}
