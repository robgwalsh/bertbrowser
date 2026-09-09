namespace BertBrowser.Core.Services.Compare;

/// <summary>
/// Settling a comparison's verdict by what the bytes actually say.
/// </summary>
/// <remarks>
/// <para>
/// A folder comparison judges on timestamps, so it can say two files are probably different but
/// never that they are the same. Reading both is the only thing that can, and this is the rule for
/// what that reading is allowed to change.
/// </para>
/// <para>
/// <strong>Which verdicts this can actually move, and which it never will.</strong>
/// <see cref="CompareVerdict.LeftNewer"/> and <see cref="CompareVerdict.RightNewer"/> are what it
/// is for: a file whose timestamp moved without its contents changing, which is most of a backup
/// drive after a restore, a sync tool or an unzip. <see cref="CompareVerdict.Unknown"/> too, where
/// a side reported no timestamp at all. <see cref="CompareVerdict.Differs"/> is handled for
/// completeness and will not move in practice — at a leaf it means equal timestamps and
/// <em>unequal sizes</em>, so the bytes cannot be identical. The rule is written over the verdict
/// rather than over that reasoning, so it stays correct if the equality rule ever changes.
/// </para>
/// <para>
/// <strong>One direction only, and only on proof.</strong> Identical bytes are stronger evidence
/// than any timestamp, so they may raise a verdict to <see cref="CompareVerdict.Same"/> —
/// including from <see cref="CompareVerdict.Unknown"/>, which is the point: an unknown that has
/// been read is no longer unknown. Everything else changes nothing. A comparison that could not
/// read a side is <see cref="ContentVerdict.Unreadable"/> and is never evidence of anything, which
/// is the same rule <see cref="CompareVerdict.Same"/>'s own documentation states — it is what
/// authorises a delete, so every doubt resolves away from it.
/// </para>
/// <para>
/// <strong>This produces a new <see cref="CompareResult"/> rather than an overlay on the view.</strong>
/// <c>SyncPlanner</c> reads its verdicts straight off the result, so a settlement kept beside the
/// rows would repaint one green while the sync went on copying it. A row that says "Same" and a
/// sync that disagrees is worse than not offering the feature.
/// </para>
/// </remarks>
public static class ContentSettlement
{
    /// <summary>What a verdict becomes once the bytes have been read.</summary>
    public static CompareVerdict Settle(CompareVerdict current, ContentVerdict content) =>
        content is ContentVerdict.Identical ? CompareVerdict.Same : current;

    /// <summary>
    /// A copy of <paramref name="result"/> with those keys settled and every folder above them
    /// re-folded.
    /// </summary>
    /// <param name="settled">A content verdict per relative key. Keys the comparison never saw are
    /// ignored rather than invented.</param>
    /// <remarks>
    /// The roll-ups are rebuilt from scratch, not patched. <see cref="CompareRules.RollUp"/> only
    /// ever raises rank, so it cannot walk a folder back down from <see cref="CompareVerdict.Differs"/>
    /// to <see cref="CompareVerdict.Same"/> when its last differing child is settled — and that
    /// folder turning green is the visible point of the whole thing. Every folder is seeded from
    /// what it is on its own, then <see cref="FolderComparer.RollUpFolders"/> — the same fold the
    /// original comparison used, rather than a second implementation of it — puts the children back.
    /// </remarks>
    public static CompareResult Apply(
        CompareResult result, IReadOnlyDictionary<string, ContentVerdict> settled)
    {
        if (settled.Count == 0) return result;

        var verdicts = new Dictionary<string, CompareVerdict>(
            result.ByRelativeKey.Count, StringComparer.Ordinal);

        foreach (var (key, verdict) in result.ByRelativeKey)
        {
            if (IsFolderOnBothSides(result, key))
            {
                // Seeded neutral; its children decide it, exactly as in the original fold.
                verdicts[key] = CompareVerdict.Same;
                continue;
            }

            verdicts[key] = settled.TryGetValue(key, out var content)
                ? Settle(verdict, content)
                : verdict;
        }

        FolderComparer.RollUpFolders(verdicts);

        // Counted exactly as FolderComparer.Compare counts — raw over every key, not through
        // CompareResult.Count, which is the ancestor-aware count the banner uses. Mixing the two
        // would quietly change what these three numbers mean the moment anything was settled.
        var same = 0;
        var unknown = 0;
        foreach (var verdict in verdicts.Values)
        {
            if (verdict is CompareVerdict.Same) same++;
            else if (verdict is CompareVerdict.Unknown) unknown++;
        }

        return new CompareResult(
            verdicts, result.Left, result.Right,
            same, verdicts.Count - same - unknown, unknown);
    }

    /// <summary>
    /// Whether a key is a folder both sides hold — the rows whose verdict is a roll-up rather than
    /// their own, and so the ones that must be seeded rather than carried over.
    /// </summary>
    private static bool IsFolderOnBothSides(CompareResult result, string key) =>
        result.Left.TryGetValue(key, out var left) && left.IsDirectory
        && result.Right.TryGetValue(key, out var right) && right.IsDirectory;
}
