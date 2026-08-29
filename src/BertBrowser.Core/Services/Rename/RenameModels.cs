namespace BertBrowser.Core.Services.Rename;

/// <summary>One item the user asked to rename.</summary>
/// <param name="Path">Full path of the item as it stands now.</param>
/// <param name="IsDirectory">True for a folder — a folder has no extension to preserve, so
/// "My.Project" renamed in a batch must not have ".Project" treated as one.</param>
/// <param name="Modified">When the item was last written, in <b>local</b> time, or null when that
/// is not known. What <c>{modified}</c> puts in a name.</param>
/// <remarks>
/// The date rides on the source rather than being read from disk by the naming rule, which is what
/// keeps <see cref="RenamePattern.Apply(IReadOnlyList{RenameSource}, RenameRule)"/> pure and its
/// tests free of the clock — the same reason <c>TransferRate</c> takes its timestamps as arguments.
/// Local rather than UTC because the file list's own Modified column is local, and a date-stamped
/// rename that disagreed with the column the user was reading would look like a bug. Null is a real
/// answer: search results arrive without a timestamp until <c>HydrateSearchMetadata</c> fills it in,
/// and a name is refused rather than stamped 0001-01-01.
/// </remarks>
public sealed record RenameSource(string Path, bool IsDirectory, DateTime? Modified = null)
{
    public string Name => System.IO.Path.GetFileName(Path);
}

/// <summary>Why the planner refused to rename one item. Every one of these is worth telling the
/// user about before anything is written — a rename that silently skips items is worse than one
/// that refuses to start.</summary>
public enum RenameRejection
{
    /// <summary>The item disappeared between selection and rename.</summary>
    SourceMissing,

    /// <summary>Drive and volume roots have no name to change.</summary>
    SourceIsRoot,

    /// <summary>The name the pattern produced is not a legal Windows file name.</summary>
    InvalidName,

    /// <summary>The name is already taken by something that isn't part of this batch, or two items
    /// in the batch would end up with the same name.</summary>
    NameTaken,

    /// <summary>A folder above this item is being renamed too, which moves this item's path out
    /// from under it. Reachable from a flattened search result, where a selection can hold both a
    /// folder and something inside it.</summary>
    InsideARenamedFolder,

    /// <summary>The rule could not produce a name — an expression that will not compile, a
    /// template with a token nobody recognises, a date format that is not one, or an item with no
    /// date for the <c>{modified}</c> the template asked for. When the rule is unusable outright
    /// this is nobody's fault in particular, so it carries an empty
    /// <see cref="RejectedRename.SourcePath"/> and belongs in the dialog's banner rather than
    /// against a row; when one item alone fell over, it names that item.</summary>
    InvalidRule,
}

/// <param name="SourcePath">The item the planner refused.</param>
/// <param name="Reason">Why.</param>
/// <param name="Message">User-facing explanation.</param>
public sealed record RejectedRename(string SourcePath, RenameRejection Reason, string Message);

/// <summary>One item the planner accepted, with the path it would end up at. Always in the same
/// folder: a rename never relocates anything.</summary>
public sealed record PlannedRename(string SourcePath, string TargetPath, bool IsDirectory)
{
    public string SourceName => Path.GetFileName(SourcePath);

    public string TargetName => Path.GetFileName(TargetPath);

    /// <summary>True when the pattern reproduced the name the item already has, down to its casing.
    /// Kept in the plan rather than dropped so a preview can show every selected item, but never
    /// executed.</summary>
    public bool IsNoOp => string.Equals(SourceName, TargetName, StringComparison.Ordinal);
}

/// <summary>The validated answer to "what would renaming these to this produce?".</summary>
public sealed record RenamePlan(
    IReadOnlyList<PlannedRename> Renames,
    IReadOnlyList<RejectedRename> Rejected)
{
    /// <summary>True when carrying the plan out would change at least one name.</summary>
    public bool HasWork => Renames.Any(r => !r.IsNoOp);

    /// <summary>The renames that actually touch disk, in the order they were planned.</summary>
    public IReadOnlyList<PlannedRename> Work => Renames.Where(r => !r.IsNoOp).ToList();

    public static RenamePlan Empty { get; } = new([], []);
}

/// <param name="SourcePath">Where the item was.</param>
/// <param name="FinalPath">Where it is now.</param>
/// <param name="IsDirectory">True for a folder.</param>
public sealed record CompletedRename(string SourcePath, string FinalPath, bool IsDirectory);

/// <param name="SourcePath">The item that could not be renamed.</param>
/// <param name="Message">The failure, phrased for the status bar.</param>
public sealed record FailedRename(string SourcePath, string Message);

/// <summary>What actually happened on disk. <see cref="Completed"/> doubles as the undo record:
/// a rename is its own inverse, so putting it back is the same operation with the paths swapped.</summary>
public sealed record RenameOutcome(
    IReadOnlyList<CompletedRename> Completed,
    IReadOnlyList<FailedRename> Failed)
{
    public bool CanUndo => Completed.Count > 0;

    public static RenameOutcome Empty { get; } = new([], []);
}
