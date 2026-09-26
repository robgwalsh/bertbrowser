using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>The six kinds of operation the history can take back and do again.</summary>
public enum UndoKind
{
    Rename,
    Move,
    /// <summary>A merged paste — the one copy whose writes are worth reversing, because it may
    /// have replaced files. A plain copy only adds, and never enters the history.</summary>
    Paste,
    Delete,
    ArchiveEdit,
    Sync,
}

public enum UndoDirection
{
    Undo,
    Redo,
}

/// <summary>How a step went, for the badge on its row.</summary>
public enum StepVerdict
{
    /// <summary>Everything in it moved.</summary>
    Clean,

    /// <summary>Some of it moved; what did not is reported, and drops out of the entry.</summary>
    Partial,

    /// <summary>Nothing moved, so the entry stayed where it was.</summary>
    Failed,
}

/// <summary>
/// How much this app is holding on the user's behalf for one entry — files a Replace displaced,
/// items deleted into the holding folder, an archive's other version.
/// </summary>
/// <param name="Complete">False when part of it could not be measured, so <see cref="Bytes"/> is a
/// floor. Counted as a floor against the budget, never as zero.</param>
public readonly record struct HeldSize(long Bytes, bool Complete)
{
    public static HeldSize None => new(0, true);

    public static HeldSize operator +(HeldSize a, HeldSize b) =>
        new(a.Bytes + b.Bytes, a.Complete && b.Complete);
}

/// <summary>One thing an entry holds: where it is parked, where it came from, and whether it is a
/// folder. The original path is what the size index knows it by.</summary>
public sealed record HeldEntry(string HeldPath, string OriginalPath, bool IsDirectory);

/// <summary>
/// How far back the history reaches: at most this many entries, and at most this much held data.
/// </summary>
public sealed record UndoBudget(int MaxEntries, long MaxHeldBytes)
{
    public const int DefaultEntries = 50;

    public const int DefaultGigabytes = 10;

    public static UndoBudget Default { get; } = FromSettings(null, null);

    /// <summary>The choices the Settings page offers.</summary>
    public static IReadOnlyList<int> EntryOptions { get; } = [10, 25, 50, 100, 200];

    /// <inheritdoc cref="EntryOptions"/>
    public static IReadOnlyList<int> GigabyteOptions { get; } = [1, 5, 10, 25, 50, 100];

    /// <summary>A budget from the two settings, where null means "never set" and takes the default.
    /// Anything below one is lifted to one: a history that cannot hold a single entry is not a
    /// setting anybody asked for.</summary>
    public static UndoBudget FromSettings(int? entries, int? gigabytes) =>
        new(Math.Max(1, entries ?? DefaultEntries), Math.Max(1L, gigabytes ?? DefaultGigabytes) << 30);
}

/// <summary>What one step did: how many items moved, and why the rest did not.</summary>
/// <param name="Note">Anything else worth saying — an elevated retry that was declined.</param>
public sealed record StepReport(int Succeeded, IReadOnlyList<string> Failures, string Note = "")
{
    public bool Clean => Failures.Count == 0;

    public bool NothingHappened => Succeeded == 0;

    public StepVerdict Verdict =>
        NothingHappened ? StepVerdict.Failed : Clean ? StepVerdict.Clean : StepVerdict.Partial;
}

/// <summary>One row of an entry's detail: an item and where it went.</summary>
public sealed record UndoItemDetail(string From, string To, bool IsDirectory);

/// <summary>
/// What a step leaves stale on screen, for the shell to reload. Core decides what changed; the App
/// decides how to show it.
/// </summary>
/// <param name="Directories">Folders whose listings changed.</param>
/// <param name="Containers">Archives whose insides changed — a tab may be standing in one.</param>
/// <param name="Vacated">Items that just went away, so a tab inside a folder among them can step
/// out of it before it is reloaded onto a missing path.</param>
/// <param name="Renamed">Renames that just happened, for the shell's own rename refresh.</param>
/// <param name="RescanCompare">A folder comparison's verdicts no longer describe the disk.</param>
public sealed record UndoRefresh(
    IReadOnlyList<string> Directories,
    IReadOnlyList<string> Containers,
    IReadOnlyList<DeletedItem> Vacated,
    IReadOnlyList<CompletedRename> Renamed,
    bool RescanCompare)
{
    public static UndoRefresh Of(IEnumerable<string?> directories) =>
        new([.. directories.OfType<string>().Where(d => d.Length > 0).Distinct(StringComparer.OrdinalIgnoreCase)],
            [], [], [], false);
}

/// <summary>A step's result: the record for the other side, what happened, and what to reload.</summary>
public sealed record StepResult(IUndoableRecord Next, StepReport Report, UndoRefresh Refresh);
