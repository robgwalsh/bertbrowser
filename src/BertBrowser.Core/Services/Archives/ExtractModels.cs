namespace BertBrowser.Core.Services.Archives;

/// <summary>What to do about a name the destination already holds.</summary>
/// <remarks>
/// <b>There is deliberately no <c>Replace</c>.</b> With only these two an extract is <em>purely
/// additive</em>, which buys the same things it buys Copy: no undo to keep, no staging, nothing
/// deleted to make room, and one fewer path to audit. It is also the honest default — overwriting
/// the destination is the single most common way people lose work to an unzip, and a file browser
/// should not make that one keystroke away.
/// </remarks>
public enum ExtractConflict
{
    /// <summary>Leave what is there and skip the entry.</summary>
    Skip,

    /// <summary>Write beside it as "name (2)", through <see cref="Paths.UniquePath"/>.</summary>
    KeepBoth,
}

public enum ExtractRejection
{
    None = 0,
    /// <summary>The container is gone, damaged, or not what its name claims.</summary>
    ArchiveUnreadable,
    /// <summary>The container needs a password nobody has given.</summary>
    PasswordRequired,
    /// <summary>The destination is missing and could not be created.</summary>
    DestinationMissing,
    /// <summary>The destination names a file rather than a folder.</summary>
    DestinationNotDirectory,
    /// <summary>Extracting here would write into the container's own path.</summary>
    DestinationInsideArchive,
    /// <summary>Nothing was selected, or nothing selected is extractable.</summary>
    NothingToExtract,
}

/// <summary>One entry, and where on disk it is going.</summary>
public sealed record PlannedExtraction(
    string EntryPath,
    string DestinationPath,
    long SizeBytes,
    bool IsDirectory);

public sealed record RejectedExtraction(ExtractRejection Reason, string Message);

/// <summary>What an extract would do. Built while a dialog is open, re-checked before it runs.</summary>
public sealed record ExtractPlan(
    string ArchiveFile,
    string DestinationDirectory,
    IReadOnlyList<PlannedExtraction> Items,
    IReadOnlyList<string> Conflicts,
    RejectedExtraction? Rejected,
    long TotalBytes,
    bool BytesAreExact)
{
    public bool HasWork => Rejected is null && Items.Count > 0;

    public static ExtractPlan Refused(ExtractRejection reason, string message) =>
        new("", "", [], [], new RejectedExtraction(reason, message), 0, false);
}

public sealed record FailedExtraction(string EntryPath, string Message);

public sealed record ExtractOutcome(
    int FilesWritten,
    long BytesWritten,
    IReadOnlyList<FailedExtraction> Failed,
    bool Cancelled)
{
    /// <summary>
    /// Extracting is additive, so there is nothing to undo — the same contract Copy and New have.
    /// The absence of a <c>CanUndo</c> member is the design, not an omission: Delete removes what
    /// this made, reversibly.
    /// </summary>
    public static ExtractOutcome Nothing => new(0, 0, [], false);
}
