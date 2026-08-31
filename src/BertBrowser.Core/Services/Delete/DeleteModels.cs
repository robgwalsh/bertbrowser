namespace BertBrowser.Core.Services.Delete;

/// <summary>What the user asked for.</summary>
public enum DeleteMode
{
    /// <summary>The ordinary delete: send the items to the Windows Recycle Bin, where they stay —
    /// visible and restorable in Explorer — until the user empties it.</summary>
    Recycle,

    /// <summary>Move the items into this app's own holding folder instead. The fallback for volumes
    /// with no working Recycle Bin, and still undoable for exactly one operation.</summary>
    Staged,

    /// <summary>Shift+Delete: erase in place, hold nothing, and there is no undo.</summary>
    Permanent,
}

/// <summary>
/// What will actually happen to one item, which is not always what the user asked for: a volume
/// with no Recycle Bin — a network share, removable media with the bin turned off — takes the
/// staged route instead. The planner decides this, so the confirmation can say what is really
/// about to happen rather than what was requested.
/// </summary>
public enum DeleteDisposition
{
    /// <summary>Into the Windows Recycle Bin.</summary>
    Recycle,

    /// <summary>Into this app's holding folder.</summary>
    Stage,

    /// <summary>Erased in place.</summary>
    Erase,
}

/// <summary>One item the user asked to delete.</summary>
/// <param name="Path">Full path of the item.</param>
/// <param name="IsDirectory">What the caller believes it is. The planner asks disk rather than
/// trusting this, but it keeps the call site honest about what was selected.</param>
public sealed record DeleteSource(string Path, bool IsDirectory)
{
    public string Name => System.IO.Path.GetFileName(Path);
}

/// <summary>Why the planner refused to delete one item. Everything except
/// <see cref="InsideADeletedFolder"/> is worth telling the user about before anything moves.</summary>
public enum DeleteRejection
{
    /// <summary>The item disappeared between selection and confirmation.</summary>
    SourceMissing,

    /// <summary>Drive and volume roots are not deletable, whatever the user selected.</summary>
    SourceIsRoot,

    /// <summary>A location the app refuses to delete outright — the Windows folder, Program Files,
    /// the user profile root. Windows stops the system ones itself now, but the profile root is
    /// entirely writable by its owner and nothing else would be in the way.</summary>
    ProtectedLocation,

    /// <summary>A folder above this item is being deleted too, so it goes with the ancestor.
    /// Reachable from a flattened search result, where a selection can hold both a folder and
    /// something inside it. An ordinary no-op, not a problem.</summary>
    InsideADeletedFolder,
}

/// <param name="SourcePath">The item the planner refused.</param>
/// <param name="Reason">Why.</param>
/// <param name="Message">User-facing explanation.</param>
public sealed record RejectedDelete(string SourcePath, DeleteRejection Reason, string Message)
{
    /// <summary>True for refusals that are ordinary no-ops rather than something to report.</summary>
    public bool IsBenign => Reason == DeleteRejection.InsideADeletedFolder;
}

/// <summary>One item the planner accepted.</summary>
/// <param name="Disposition">Where this particular item is going. Defaults to
/// <see cref="DeleteDisposition.Stage"/>, which is the behaviour that predates the Recycle Bin and
/// the safe answer for any caller that has not thought about it.</param>
public sealed record PlannedDelete(
    string SourcePath, bool IsDirectory, DeleteDisposition Disposition = DeleteDisposition.Stage)
{
    public string Name => Path.GetFileName(SourcePath);

    /// <summary>The folder the item will vanish from — what a listing has to reload afterwards.</summary>
    public string ParentPath => Path.GetDirectoryName(SourcePath) ?? "";
}

/// <summary>The validated answer to "what would deleting these do?".</summary>
/// <param name="Mode">What the user asked for. Individual items may still differ — see
/// <see cref="PlannedDelete.Disposition"/>.</param>
public sealed record DeletePlan(
    DeleteMode Mode,
    IReadOnlyList<PlannedDelete> Deletions,
    IReadOnlyList<RejectedDelete> Rejected)
{
    /// <summary>A Shift+Delete: the items are erased instead of being set aside, and there is no
    /// undo. Kept as a computed property because it is what the dialog and the executor's erase
    /// branch actually care about.</summary>
    public bool Permanent => Mode == DeleteMode.Permanent;

    public bool HasWork => Deletions.Count > 0;

    /// <summary>Refusals worth surfacing; no-ops are filtered out.</summary>
    public IReadOnlyList<RejectedDelete> Problems => Rejected.Where(r => !r.IsBenign).ToList();

    /// <summary>True when the user asked for the Recycle Bin but at least one item cannot go there
    /// and will be held by this app instead. The confirmation says so rather than letting the
    /// difference be discovered later.</summary>
    public bool HasStagedFallback =>
        Mode == DeleteMode.Recycle && Deletions.Any(d => d.Disposition == DeleteDisposition.Stage);

    public static DeletePlan Empty(DeleteMode mode) => new(mode, [], []);
}

/// <summary>How much one planned item actually amounts to, so the confirmation can say what is
/// about to go rather than just how many rows were selected.</summary>
/// <param name="Incomplete">True when part of the tree could not be read, so the totals are a
/// floor rather than the answer.</param>
public sealed record DeleteMeasurement(
    string SourcePath,
    bool IsDirectory,
    long Bytes,
    int Files,
    int Directories,
    bool Incomplete);

/// <summary>Every measurement for a plan, plus the totals the dialog leads with.</summary>
public sealed record DeleteSurvey(IReadOnlyList<DeleteMeasurement> Items)
{
    public long Bytes => Items.Sum(i => i.Bytes);

    public int Files => Items.Sum(i => i.Files);

    public int Directories => Items.Sum(i => i.Directories);

    public bool Incomplete => Items.Any(i => i.Incomplete);

    public static DeleteSurvey Empty { get; } = new([]);
}

/// <summary>One item that is no longer where it was.</summary>
/// <param name="SourcePath">Where it was — the undo target.</param>
/// <param name="IsDirectory">True for a folder.</param>
/// <param name="StagedPath">Where it is being held until the delete is committed, or null when it
/// was erased outright or sent to the Recycle Bin. Undo moves it back from here.</param>
/// <param name="RecycledPath">The item's <c>$R</c> path inside the Recycle Bin, when that is where
/// it went. Undo restores from exactly this rather than searching the bin for something with a
/// matching original path, which is what keeps it correct when the same path has been deleted
/// more than once.</param>
public sealed record DeletedItem(
    string SourcePath, bool IsDirectory, string? StagedPath, string? RecycledPath = null)
{
    public string Name => Path.GetFileName(SourcePath);

    /// <summary>True when the item still exists somewhere and can be put back.</summary>
    public bool IsRecoverable => StagedPath is not null || RecycledPath is not null;
}

/// <param name="SourcePath">The item that could not be deleted.</param>
/// <param name="Message">The failure, phrased for the status bar.</param>
/// <param name="AccessDenied">Windows refused permission, rather than the item being missing or in
/// use. The only failure an administrator token could fix.</param>
public sealed record FailedDelete(string SourcePath, string Message, bool AccessDenied = false);

/// <summary>What actually happened on disk. <see cref="Deleted"/> doubles as the undo record:
/// each entry knows where its data is being held.</summary>
/// <param name="StagingDirectories">Every holding folder this run created, so the whole lot can be
/// discarded in one go once the delete can no longer be undone.</param>
public sealed record DeleteOutcome(
    bool Permanent,
    IReadOnlyList<DeletedItem> Deleted,
    IReadOnlyList<FailedDelete> Failed,
    IReadOnlyList<string> StagingDirectories)
{
    /// <summary>Only a delete that still has the data somewhere can be reversed. A permanent one
    /// erased it; so did the Recycle Bin for any item it declined to hold, which is why this asks
    /// each item rather than trusting the mode.</summary>
    public bool CanUndo => !Permanent && Deleted.Any(d => d.IsRecoverable);

    public static DeleteOutcome Empty(bool permanent) => new(permanent, [], [], []);
}

/// <summary>Restoring a delete: how many items came back, and what could not.</summary>
public sealed record DeleteUndoResult(int Restored, IReadOnlyList<FailedDelete> Failed);

/// <summary>Progress for the status bar while a delete runs.</summary>
public sealed record DeleteProgress(int Done, int Total, string CurrentName);
