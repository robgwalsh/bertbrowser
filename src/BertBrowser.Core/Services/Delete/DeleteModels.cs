namespace BertBrowser.Core.Services.Delete;

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
    /// the user profile root. This app runs elevated for its MFT index, so the usual "Windows will
    /// stop you" backstop is not there.</summary>
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
public sealed record PlannedDelete(string SourcePath, bool IsDirectory)
{
    public string Name => Path.GetFileName(SourcePath);

    /// <summary>The folder the item will vanish from — what a listing has to reload afterwards.</summary>
    public string ParentPath => Path.GetDirectoryName(SourcePath) ?? "";
}

/// <summary>The validated answer to "what would deleting these do?".</summary>
/// <param name="Permanent">True for a Shift+Delete: the items are erased instead of being set
/// aside, and there is no undo.</param>
public sealed record DeletePlan(
    bool Permanent,
    IReadOnlyList<PlannedDelete> Deletions,
    IReadOnlyList<RejectedDelete> Rejected)
{
    public bool HasWork => Deletions.Count > 0;

    /// <summary>Refusals worth surfacing; no-ops are filtered out.</summary>
    public IReadOnlyList<RejectedDelete> Problems => Rejected.Where(r => !r.IsBenign).ToList();

    public static DeletePlan Empty(bool permanent) => new(permanent, [], []);
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
/// was erased outright. Undo moves it back from here.</param>
public sealed record DeletedItem(string SourcePath, bool IsDirectory, string? StagedPath)
{
    public string Name => Path.GetFileName(SourcePath);
}

/// <param name="SourcePath">The item that could not be deleted.</param>
/// <param name="Message">The failure, phrased for the status bar.</param>
public sealed record FailedDelete(string SourcePath, string Message);

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
    /// <summary>Only a staged delete can be reversed; a permanent one erased the data.</summary>
    public bool CanUndo => !Permanent && Deleted.Count > 0;

    public static DeleteOutcome Empty(bool permanent) => new(permanent, [], [], []);
}

/// <summary>Restoring a delete: how many items came back, and what could not.</summary>
public sealed record DeleteUndoResult(int Restored, IReadOnlyList<FailedDelete> Failed);

/// <summary>Progress for the status bar while a delete runs.</summary>
public sealed record DeleteProgress(int Done, int Total, string CurrentName);
