using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Elevation;

/// <summary>Which of the four mutations — or which undo of one — the helper is being asked for.</summary>
/// <remarks>
/// There is deliberately no <c>RenameUndo</c>: a rename is its own inverse, so
/// <c>RenameExecutor.UndoPlan</c> is computed app-side and sent as an ordinary
/// <see cref="Rename"/>. Archives are absent too, and that is a decision rather than an omission —
/// rewriting a container means handing a third-party decoder attacker-controlled bytes, which is
/// not something to do with a token in hand.
/// </remarks>
public enum ElevationOperation
{
    TransferMove,
    TransferCopy,
    Delete,
    Rename,
    NewItem,
    TransferUndo,
    DeleteUndo,
}

/// <summary>The <c>Begin</c> line: everything about a request that is not one of its items.</summary>
public sealed record ElevationHeader(
    ElevationOperation Operation,
    string DestinationDirectory = "",
    DeleteMode DeleteMode = DeleteMode.Recycle,
    bool Permanent = false);

/// <summary>
/// One item of a transfer request, carrying its conflict resolution rather than leaving the
/// resolutions in a dictionary on the header.
/// </summary>
/// <remarks>
/// Two reasons, and the first is the load-bearing one. A dictionary on the header would put the
/// whole selection on one line, which is exactly the bound <c>ElevationProtocol</c> refuses to
/// raise. And the resolution has to travel at all, or a <c>Replace</c> the user chose silently
/// becomes the default <c>KeepBoth</c> on the second pass and the operation quietly changes meaning
/// half way through.
/// </remarks>
public sealed record ElevationTransferItem(PlannedTransfer Item, ConflictResolution Resolution);

/// <summary>The <c>Done</c> line for a transfer. A skipped item produces neither a completion nor a
/// failure, so it needs somewhere of its own to be reported.</summary>
public sealed record ElevationTransferResult(CompletedTransfer? Completed, string? Skipped);

/// <summary>The <c>Done</c> line for a creation — the one operation whose success is a bare path.</summary>
public sealed record ElevationNewItemResult(string CreatedPath);

/// <summary>A <c>Progress</c> line: the superset of what a transfer and a delete each report, so one
/// shape covers both.</summary>
public sealed record ElevationProgressReport(
    int Done,
    int Total,
    string CurrentName,
    long BytesDone = 0,
    long CurrentItemBytes = 0,
    long CurrentItemTotal = 0);

/// <summary>The <c>End</c> line: what an outcome needs that is not per-item.</summary>
public sealed record ElevationSummary(
    bool Cancelled,
    IReadOnlyList<string> StagingDirectories,
    int Restored = 0);

/// <summary>How an attempt to run something elevated ended.</summary>
public enum ElevationStatus
{
    /// <summary>The helper ran and reported an outcome. That outcome may still contain failures —
    /// this only says the elevated pass happened.</summary>
    Completed,

    /// <summary>The user answered the UAC prompt with No. Their choice, not a failure.</summary>
    Declined,

    /// <summary>This account cannot elevate, so no prompt was raised. Asked before the dialog is
    /// shown, never after: a standard user offered a shield gets a credential prompt for somebody
    /// else's password, which is worse than never being offered one.</summary>
    NotAdministrator,

    /// <summary>The helper was missing, could not be started, or died mid-operation.</summary>
    Unavailable,
}

/// <summary>What came back from an elevated attempt.</summary>
/// <param name="Detail">Phrased for the status bar; empty when nothing needs saying.</param>
public sealed record ElevatedRun<T>(ElevationStatus Status, T? Result, string Detail = "")
{
    public static ElevatedRun<T> Ran(T result) => new(ElevationStatus.Completed, result);
    public static readonly ElevatedRun<T> Declined = new(ElevationStatus.Declined, default,
        "permission was declined");
    public static readonly ElevatedRun<T> NotAdministrator = new(ElevationStatus.NotAdministrator, default,
        "this account cannot provide administrator permission");
    public static ElevatedRun<T> Unavailable(string detail) =>
        new(ElevationStatus.Unavailable, default, detail);
}
