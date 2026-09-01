namespace BertBrowser.Core.Services.Compare;

/// <summary>What comparing two trees found.</summary>
/// <param name="ByRelativeKey">A verdict for every path either side holds, folders included.</param>
/// <param name="Left">The left listing, keyed the same way.</param>
/// <param name="Right">The right listing.</param>
/// <param name="SameCount">Entries that match.</param>
/// <param name="DifferenceCount">Entries that do not, excluding the ones nothing is known about.</param>
/// <param name="UnknownCount">Entries that could not be compared. Counted separately because they
/// are neither a match nor something a sync may act on.</param>
public sealed record CompareResult(
    IReadOnlyDictionary<string, CompareVerdict> ByRelativeKey,
    IReadOnlyDictionary<string, CompareEntry> Left,
    IReadOnlyDictionary<string, CompareEntry> Right,
    int SameCount,
    int DifferenceCount,
    int UnknownCount)
{
    public bool AnyDifference => DifferenceCount > 0 || UnknownCount > 0;

    /// <summary>The verdict for a path, or <see cref="CompareVerdict.Unknown"/> when the comparison
    /// never saw it — a file created since the scan has not been compared, and saying so is the
    /// only honest answer.</summary>
    public CompareVerdict For(string relativeKey) =>
        ByRelativeKey.TryGetValue(relativeKey, out var verdict) ? verdict : CompareVerdict.Unknown;

    public static readonly CompareResult None = new(
        new Dictionary<string, CompareVerdict>(StringComparer.Ordinal),
        new Dictionary<string, CompareEntry>(StringComparer.Ordinal),
        new Dictionary<string, CompareEntry>(StringComparer.Ordinal),
        0, 0, 0);
}

/// <summary>
/// Pairing two flat subtree listings and judging every pair.
/// </summary>
/// <remarks>
/// <para>
/// Shaped after <see cref="FileListDiff"/>, which does the same job for one folder across time:
/// pure, keyed on a canonical path, and returning a description of the difference rather than
/// acting on it. Everything that decides what a difference <em>means</em> is in
/// <see cref="CompareEquality"/> and <see cref="CompareRules"/>; this class only pairs and folds.
/// </para>
/// <para>
/// A folder's verdict is the roll-up of everything beneath it, which is what lets a whole tree be
/// synced without opening it. The fold needs no recursion and no particular input order: each
/// entry's verdict is merged into each of its ancestors, and <see cref="CompareRules.RollUp"/> is
/// order-independent.
/// </para>
/// </remarks>
public static class FolderComparer
{
    public static CompareResult Compare(
        IReadOnlyList<CompareEntry> left,
        IReadOnlyList<CompareEntry> right,
        CompareTolerance tolerance)
    {
        ArgumentNullException.ThrowIfNull(left);
        ArgumentNullException.ThrowIfNull(right);

        var leftByKey = Index(left);
        var rightByKey = Index(right);

        var verdicts = new Dictionary<string, CompareVerdict>(
            leftByKey.Count + rightByKey.Count, StringComparer.Ordinal);

        foreach (var (key, entry) in leftByKey)
        {
            verdicts[key] = CompareEquality.Verdict(
                entry, rightByKey.TryGetValue(key, out var other) ? other : null, tolerance);
        }

        foreach (var (key, entry) in rightByKey)
        {
            if (leftByKey.ContainsKey(key)) continue; // already judged from the left
            verdicts[key] = CompareEquality.Verdict(null, entry, tolerance);
        }

        RollUpFolders(verdicts);

        var same = 0;
        var unknown = 0;
        foreach (var verdict in verdicts.Values)
        {
            if (verdict is CompareVerdict.Same) same++;
            else if (verdict is CompareVerdict.Unknown) unknown++;
        }

        return new CompareResult(
            verdicts, leftByKey, rightByKey,
            same, verdicts.Count - same - unknown, unknown);
    }

    /// <summary>
    /// Merges every entry's verdict into each of its ancestors. Reads the verdicts settled by
    /// pairing and writes only to ancestor keys, so no entry is ever folded into itself and the
    /// pass does not depend on the order the dictionary happens to enumerate in.
    /// </summary>
    private static void RollUpFolders(Dictionary<string, CompareVerdict> verdicts)
    {
        var direct = verdicts.ToArray();
        foreach (var (key, verdict) in direct)
        {
            foreach (var ancestor in CompareKeys.Ancestors(key))
            {
                // A listing that omits a folder row but holds its contents still gets a verdict for
                // the folder, seeded neutral so its children are what decide it.
                var folder = verdicts.TryGetValue(ancestor, out var existing)
                    ? existing
                    : CompareVerdict.Same;
                verdicts[ancestor] = CompareRules.RollUp(folder, verdict);
            }
        }
    }

    private static Dictionary<string, CompareEntry> Index(IReadOnlyList<CompareEntry> entries)
    {
        var byKey = new Dictionary<string, CompareEntry>(entries.Count, StringComparer.Ordinal);
        foreach (var entry in entries)
            byKey[entry.RelativeKey] = entry;
        return byKey;
    }
}
