using System.IO.Enumeration;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

/// <summary>One filesystem entry produced by <see cref="FileSystemWalker"/>.</summary>
internal readonly record struct WalkEntry(
    string DisplayPath,
    string PathKey,
    string Name,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc);

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
    public static void Walk(string root, Func<WalkEntry, bool> onEntry, CancellationToken ct)
    {
        var stack = new Stack<(string Path, string Key)>();
        stack.Push((PathKey.NormalizeDisplay(root), PathKey.Canonicalize(root)));

        while (stack.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            var (dirPath, dirKey) = stack.Pop();

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
                        var walkEntry = new WalkEntry(
                            entry.ToFullPath(),
                            keyPrefix + name.ToUpperInvariant(),
                            name,
                            isDir,
                            isDir ? 0 : entry.Length,
                            entry.LastWriteTimeUtc.UtcDateTime);
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
                    if (!onEntry(entry))
                        return;
                    if (descend)
                        stack.Push((entry.DisplayPath, entry.PathKey));
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
