namespace BertBrowser.Core.Services.Archives;

/// <summary>One change to make to a container's contents.</summary>
public abstract record ArchiveEdit;

/// <summary>Remove an entry, and everything under it if it is a folder.</summary>
public sealed record RemoveEntry(string EntryPath) : ArchiveEdit;

/// <summary>Give an entry a new name in the same folder.</summary>
public sealed record RenameEntry(string EntryPath, string NewName) : ArchiveEdit;

/// <summary>Put a file from disk into the container at <paramref name="EntryPath"/>.</summary>
public sealed record AddFile(string SourcePath, string EntryPath) : ArchiveEdit;

public enum ArchiveEditRejection
{
    None = 0,
    /// <summary>The container is gone, damaged, or not what its name claims.</summary>
    Unreadable,
    /// <summary>Nothing in the dependency graph can write this format.</summary>
    FormatNotWritable,
    /// <summary>Solid: rewriting would have to recompress every block, and cannot preserve it.</summary>
    Solid,
    /// <summary>Encrypted: a rewrite would silently drop the encryption.</summary>
    Encrypted,
    /// <summary>Truncated or missing volumes — a rewrite would make the loss permanent.</summary>
    Incomplete,
    /// <summary>Larger than this app will rewrite in one go.</summary>
    TooLarge,
    /// <summary>An entry named by the edit is not in the container.</summary>
    EntryMissing,
    /// <summary>Two entries would end up with the same path.</summary>
    NameTaken,
    /// <summary>The new name is not one Windows would accept.</summary>
    InvalidName,
    /// <summary>Nothing to do.</summary>
    NothingToDo,
}

public sealed record RejectedArchiveEdit(ArchiveEditRejection Reason, string Message);

/// <summary>
/// What editing a container would amount to, and what it will cost.
/// </summary>
/// <remarks>
/// <see cref="RewriteBytes"/> is the whole archive, not the change: nothing can modify one of these
/// formats in place, so deleting one entry from a 4 GB zip reads and writes 4 GB. The dialog says
/// so before anyone agrees to it.
/// </remarks>
public sealed record ArchiveEditPlan(
    string ArchiveFile,
    IReadOnlyList<ArchiveEdit> Edits,
    IReadOnlyDictionary<string, string> Renames,
    IReadOnlySet<string> Removals,
    IReadOnlyList<AddFile> Additions,
    long RewriteBytes,
    RejectedArchiveEdit? Rejected)
{
    public bool HasWork => Rejected is null && Edits.Count > 0;

    public static ArchiveEditPlan Refused(ArchiveEditRejection reason, string message) =>
        new("", [], new Dictionary<string, string>(), new HashSet<string>(), [], 0,
            new RejectedArchiveEdit(reason, message));
}

/// <summary>
/// What an archive edit did, and what it left behind so it can be undone.
/// </summary>
/// <param name="StagedOriginal">
/// Where the container that was replaced is being held. Erased by <c>CommitStaging</c> exactly one
/// operation later, the same contract a Replace's staging has — so the data outlives the undo
/// record rather than the other way round.
/// </param>
public sealed record ArchiveEditOutcome(
    string ArchiveFile,
    string? StagedOriginal,
    int EntriesWritten,
    string? Failure,
    bool Cancelled)
{
    public bool CanUndo => StagedOriginal is not null && Failure is null && !Cancelled;

    public static ArchiveEditOutcome Nothing(string archiveFile) =>
        new(archiveFile, null, 0, null, false);
}
