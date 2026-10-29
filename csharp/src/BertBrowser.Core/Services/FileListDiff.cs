using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

/// <summary>What changed in a folder between two listings.</summary>
/// <param name="Removed">Canonical path keys that are no longer there.</param>
/// <param name="Added">Entries that were not there before.</param>
/// <param name="Updated">
/// Entries at the same path whose displayed details differ — a size, a timestamp, an attribute, or
/// the casing of the name.
/// </param>
public sealed record FileListChanges(
    IReadOnlyList<string> Removed,
    IReadOnlyList<FileEntry> Added,
    IReadOnlyList<FileEntry> Updated)
{
    public bool Any => Removed.Count > 0 || Added.Count > 0 || Updated.Count > 0;

    public static readonly FileListChanges None = new([], [], []);
}

/// <summary>
/// Comparing two listings of the same folder, so a refresh can change what actually changed.
/// </summary>
/// <remarks>
/// <para>
/// This exists because of what the alternative costs. The file list replaces its whole collection
/// on a load, and the view focuses the list when that happens — fine for a navigation the user
/// asked for, ruinous for a refresh nobody asked for: it would drop the selection, jump the scroll
/// position back to the top, and pull the caret out of another pane's search box every time a file
/// appeared on disk. The folder tree already hit exactly this and answered it the same way, by
/// diffing rather than clearing.
/// </para>
/// <para>
/// Pure, and keyed on <see cref="PathKey.Canonicalize"/> so the comparison agrees with everything
/// else in the app about what "the same path" means.
/// </para>
/// </remarks>
public static class FileListDiff
{
    public static FileListChanges Compute(IReadOnlyList<FileEntry> current, IReadOnlyList<FileEntry> next)
    {
        ArgumentNullException.ThrowIfNull(current);
        ArgumentNullException.ThrowIfNull(next);

        var before = new Dictionary<string, FileEntry>(current.Count, StringComparer.Ordinal);
        foreach (var entry in current)
            before[PathKey.Canonicalize(entry.FullPath)] = entry;

        var added = new List<FileEntry>();
        var updated = new List<FileEntry>();
        var seen = new HashSet<string>(next.Count, StringComparer.Ordinal);

        foreach (var entry in next)
        {
            var key = PathKey.Canonicalize(entry.FullPath);
            if (!seen.Add(key)) continue; // a listing cannot hold one path twice; ignore if it does

            if (!before.TryGetValue(key, out var existing))
                added.Add(entry);
            else if (Differs(existing, entry))
                updated.Add(entry);
        }

        var removed = new List<string>();
        foreach (var key in before.Keys)
        {
            if (!seen.Contains(key)) removed.Add(key);
        }

        return removed.Count == 0 && added.Count == 0 && updated.Count == 0
            ? FileListChanges.None
            : new FileListChanges(removed, added, updated);
    }

    /// <summary>
    /// Whether anything the row shows has changed.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <see cref="FileEntry.Name"/> is compared <b>ordinally</b>, and that is the interesting case:
    /// path keys are uppercased, so a rename that only changes casing — "notes.txt" to "Notes.txt"
    /// — is the same key and would otherwise register as no change at all, leaving the list showing
    /// the old spelling until something else forced a reload.
    /// </para>
    /// <para>
    /// This once weighed <b>only</b> the Hidden attribute, because that was the only one a row
    /// rendered and because a row rebuilt from a listing could only reconstruct the flags it showed
    /// — so a full comparison called every row different on every pass. Configurable columns make
    /// the first half false, and the row carrying its whole <see cref="FileEntry.Attributes"/>
    /// makes the second half false, so the set below can be compared honestly.
    /// </para>
    /// <para>
    /// <b><see cref="FileEntry.AccessedUtc"/> is deliberately not compared</b>, though a column may
    /// render it. Reading a file moves it, and this app reads files constantly — the preview pane,
    /// content search, and the shell property reads behind a metadata column all do. Weighing it
    /// would mark every row the user merely looked at as changed on the next pass, which is exactly
    /// the churn the original rule was written to avoid, arriving through a door columns opened.
    /// </para>
    /// <para>
    /// <see cref="MeaningfulAttributes"/> is the rest of that reasoning: the cloud-provider bits
    /// move on files nobody touched, and <c>Normal</c> is only meaningful in isolation, so a lister
    /// reporting it one pass and <c>Archive</c> the next would look like a change. <c>Archive</c>
    /// itself stays in, because a write that sets it moves <see cref="FileEntry.ModifiedUtc"/> too
    /// and it costs nothing, while an <c>attrib +a</c> on its own is a real change to a real column.
    /// </para>
    /// </remarks>
    private static bool Differs(FileEntry a, FileEntry b) =>
        !string.Equals(a.Name, b.Name, StringComparison.Ordinal) ||
        a.SizeBytes != b.SizeBytes ||
        a.ModifiedUtc != b.ModifiedUtc ||
        a.CreatedUtc != b.CreatedUtc ||
        a.IsDirectory != b.IsDirectory ||
        (a.Attributes & MeaningfulAttributes) != (b.Attributes & MeaningfulAttributes);

    /// <summary>The attributes a row can render and that only change when something really
    /// happened to the file.</summary>
    private const FileAttributes MeaningfulAttributes =
        FileAttributes.ReadOnly | FileAttributes.Hidden | FileAttributes.System |
        FileAttributes.Directory | FileAttributes.Archive | FileAttributes.Compressed |
        FileAttributes.Encrypted | FileAttributes.ReparsePoint;
}
