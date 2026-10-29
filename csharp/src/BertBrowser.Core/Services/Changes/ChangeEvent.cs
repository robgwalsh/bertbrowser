namespace BertBrowser.Core.Services.Changes;

/// <summary>
/// One change the USN tail resolved, on its way into <c>fs_change</c>.
/// </summary>
/// <param name="PathKey"><c>PathKey.Canonicalize</c> of the path, as every keyed table stores it.</param>
/// <param name="DisplayPath">The path as cased on disk. Stored, unlike <c>fs_entry</c>'s, because a
/// deleted file has no ancestors left to rebuild it from.</param>
/// <param name="Hidden">Effective — the entry's own Hidden bit OR'd down from every ancestor, the
/// same flag <c>fs_entry.hidden</c> carries, so the timeline can honour "show hidden items".</param>
/// <param name="OldDisplayPath">For a rename, where it was; null otherwise.</param>
/// <param name="Utc">The record's own timestamp, not the time it was written down.</param>
public sealed record ChangeEvent(
    string PathKey,
    string DisplayPath,
    bool IsDirectory,
    bool Hidden,
    ChangeKind Kind,
    string? OldDisplayPath,
    DateTime Utc);
