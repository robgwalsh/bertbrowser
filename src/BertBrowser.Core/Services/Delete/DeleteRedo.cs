namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// The plan that deletes again what an undo put back.
/// </summary>
/// <remarks>
/// Each item goes the way it went the first time: back to the Recycle Bin if that is where it was,
/// otherwise into the holding folder. The executor still re-asks whether the bin will take it, and
/// falls back to holding rather than erasing — a redo is never the thing that makes a delete
/// permanent.
/// </remarks>
public static class DeleteRedo
{
    public static DeletePlan PlanFor(IReadOnlyList<DeletedItem> reverted)
    {
        // The undo walked them in reverse; the redo goes in the order they were first deleted.
        var deletions = reverted
            .Reverse()
            .Select(d => new PlannedDelete(
                d.SourcePath, d.IsDirectory,
                d.RecycledPath is not null ? DeleteDisposition.Recycle : DeleteDisposition.Stage))
            .ToList();

        var mode = deletions.Any(d => d.Disposition == DeleteDisposition.Recycle)
            ? DeleteMode.Recycle
            : DeleteMode.Staged;
        return new DeletePlan(mode, deletions, []);
    }
}
