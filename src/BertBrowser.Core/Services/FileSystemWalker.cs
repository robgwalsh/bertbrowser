using System.IO.Enumeration;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

/// <summary>One filesystem entry produced by <see cref="FileSystemWalker"/>.
/// <see cref="Hidden"/> is effective (the entry's own Hidden attribute or that of any
/// ancestor within the walked subtree).</summary>
internal readonly record struct WalkEntry(
    string DisplayPath,
    string PathKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    bool Hidden);

/// <summary>
/// Iterative pre-order walk of a directory subtree (explicit stack, so deep trees
/// cannot overflow). The root itself is not emitted. Reparse-point directories
/// (junctions/symlinks) are emitted but never descended into, so cycles are
/// impossible. Inaccessible directories are skipped rather than fatal. Sizes and
/// timestamps come straight from the OS find data — no extra stat calls.
/// </summary>
internal static class FileSystemWalker
{
    /// <param name="onEntry">Invoked for every entry; return false to stop the walk.</param>
    /// <param name="includeHidden">When false, hidden entries (and the subtrees under
    /// hidden directories) are skipped entirely. When true, every entry is emitted with
    /// its effective <see cref="WalkEntry.Hidden"/> flag set.</param>
    /// <param name="rootHidden">Seed for the root's hidden state — set when the walked
    /// root itself lives inside a hidden subtree (used by incremental mini-crawls).</param>
    public static void Walk(string root, Func<WalkEntry, bool> onEntry, CancellationToken ct,
        bool includeHidden, bool rootHidden = false)
    {
        var stack = new Stack<(string Path, string Key, bool Hidden)>();
        stack.Push((PathKey.NormalizeDisplay(root), PathKey.Canonicalize(root), rootHidden));

        while (stack.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            var (dirPath, dirKey, dirHidden) = stack.Pop();

            // Drive roots canonicalize to "C:\" — avoid a doubled separator.
            var keyPrefix = dirKey[^1] == '\\' ? dirKey : dirKey + '\\';

            try
            {
                var enumerable = new FileSystemEnumerable<(WalkEntry Entry, bool Descend)>(
                    dirPath,
                    (ref FileSystemEntry entry) =>
                    {
                        var isDir = entry.IsDirectory;
                        var name = entry.FileName.ToString();
                        // Effective hidden: this entry, or any ancestor within the subtree.
                        var hidden = dirHidden || (entry.Attributes & FileAttributes.Hidden) != 0;
                        var walkEntry = new WalkEntry(
                            entry.ToFullPath(),
                            keyPrefix + name.ToUpperInvariant(),
                            name,
                            isDir,
                            isDir ? 0 : entry.Length,
                            entry.LastWriteTimeUtc.UtcDateTime,
                            hidden);
                        var descend = isDir && (entry.Attributes & FileAttributes.ReparsePoint) == 0;
                        return (walkEntry, descend);
                    },
                    new EnumerationOptions
                    {
                        IgnoreInaccessible = false,
                        AttributesToSkip = 0,
                        RecurseSubdirectories = false,
                    });

                foreach (var (entry, descend) in enumerable)
                {
                    if (!includeHidden && entry.Hidden)
                        continue; // skip hidden entries and everything beneath them
                    if (!onEntry(entry))
                        return;
                    if (descend)
                        stack.Push((entry.DisplayPath, entry.PathKey, entry.Hidden));
                }
            }
            catch (UnauthorizedAccessException)
            {
                // Skip inaccessible branches; search is best-effort like the size scanner.
            }
            catch (IOException)
            {
            }
        }
    }
}
