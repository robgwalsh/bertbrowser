using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// The plan that does an undone transfer again, built from what the undo actually put back.
/// </summary>
/// <remarks>
/// <para>
/// <b>Every item lands exactly where it landed the first time.</b> Each one carries its old final
/// path as its destination, so a Keep-both that produced <c>img (2).jpg</c> produces it again rather
/// than a fresh number, and an expanded merge's items land deep in the tree rather than at the top.
/// </para>
/// <para>
/// <b>Only a name the first run displaced may be displaced again</b> — Replace for a move,
/// Overwrite for a copy, since that is what the undo just put back there. Everything else is Skip.
/// A redo never numbers a newcomer and never displaces something it has no record of: a name taken
/// since the undo is reported and left alone, because nobody chose to replace it.
/// </para>
/// </remarks>
public static class TransferRedo
{
    public static (TransferPlan Plan, IReadOnlyDictionary<string, ConflictResolution> Resolutions) PlanFor(
        TransferVerb verb,
        string destinationDirectory,
        IReadOnlyList<CompletedTransfer> reverted,
        IReadOnlyList<string> prunedDirectories)
    {
        var transfers = new List<PlannedTransfer>(reverted.Count);
        var resolutions = new Dictionary<string, ConflictResolution>(StringComparer.Ordinal);
        var displace = verb == TransferVerb.Move ? ConflictResolution.Replace : ConflictResolution.Overwrite;

        // The undo walked them in reverse, so its list is newest-first; the redo wants the original
        // order back, so a folder is written before anything that was copied into it.
        foreach (var item in reverted.Reverse())
        {
            var displaced = item.DisplacedStagePath is not null;
            transfers.Add(new PlannedTransfer(item.SourcePath, item.IsDirectory, item.FinalPath, displaced));
            resolutions[PathKey.Canonicalize(item.SourcePath)] =
                displaced ? displace : ConflictResolution.Skip;
        }

        var plan = new TransferPlan(verb, destinationDirectory, transfers, [])
        {
            PruneDirectories = verb == TransferVerb.Move ? prunedDirectories : [],
        };
        return (plan, resolutions);
    }
}
