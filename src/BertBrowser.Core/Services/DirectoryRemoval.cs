namespace BertBrowser.Core.Services;

/// <summary>
/// Erasing a directory tree, which this app has to do in a way <c>Directory.Delete(recursive: true)</c>
/// does not.
/// </summary>
/// <remarks>
/// <para>
/// <b>That call cannot be used on anything the user owns.</b> Given a tree with a junction anywhere
/// in it, it erases everything else in the tree and <em>then</em> throws
/// <c>ERROR_INVALID_PARAMETER</c> naming the link. Both consequences are bad and the pairing is
/// worse: on a permanent delete the folder's contents were destroyed, could not be undone, and the
/// user was told the delete had failed; on a staging commit the throw is swallowed as harmless
/// cleanup and half a folder is left behind for good.
/// </para>
/// <para>
/// So the walk is done here, once, for every caller that erases a tree — the delete executor's
/// permanent erase and both executors' staging commits. A junction is removed as the single entry it
/// is, never followed, which is the same reading <c>DeleteSurveyor</c> and <c>TransferPlanner</c>
/// already take of one.
/// </para>
/// </remarks>
public static class DirectoryRemoval
{
    /// <summary>
    /// Erases <paramref name="root"/> and everything under it. Throws exactly what the underlying
    /// filesystem calls throw — a caller that wants best-effort cleanup catches, and one carrying out
    /// a delete the user asked for reports.
    /// </summary>
    /// <remarks>
    /// Walked with an explicit stack rather than by recursion, because the depth is whatever the
    /// user's disk contains. Directories are collected in pre-order — every parent before its
    /// children — and removed in reverse, which is the post-order the removal needs.
    /// </remarks>
    public static void RemoveTree(string root)
    {
        var top = new DirectoryInfo(root);
        if (IsLink(top))
        {
            // Non-recursive on purpose: this removes the link, never what it points at.
            top.Delete(recursive: false);
            return;
        }

        var directories = new List<string> { root };
        var pending = new Stack<DirectoryInfo>();
        pending.Push(top);

        while (pending.Count > 0)
        {
            var current = pending.Pop();
            foreach (var entry in current.GetFileSystemInfos())
            {
                if (entry is DirectoryInfo child && !IsLink(child))
                {
                    pending.Push(child);
                    directories.Add(child.FullName);
                }
                else
                {
                    // A file, or a link of either kind: one entry to remove, and for a directory
                    // link FileSystemInfo.Delete is the non-recursive overload.
                    entry.Delete();
                }
            }
        }

        for (var i = directories.Count - 1; i >= 0; i--)
            Directory.Delete(directories[i], recursive: false);
    }

    /// <summary>A junction or symlink: one entry, not the tree it points at.</summary>
    public static bool IsLink(FileSystemInfo info) =>
        (info.Attributes & FileAttributes.ReparsePoint) != 0;
}
