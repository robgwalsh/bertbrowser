namespace BertBrowser.Core.Services;

/// <summary>
/// Swapping a rewritten file in for the one it replaces, keeping the old one beside it.
/// </summary>
/// <remarks>
/// <para>
/// <b><c>ReplaceFile</c>, not two renames.</b> A file written fresh beside the original is a new
/// file: it has today's creation time, the folder's inherited permissions instead of its own, none
/// of its alternate streams (the mark that says a download came from the internet is one) and none
/// of its attributes. Renaming it into place quietly swaps all of that along with the content.
/// <c>ReplaceFile</c> is the call Windows has for exactly this — it carries the replaced file's
/// identity onto the replacement — and it leaves the old content under the backup name in the same
/// step, which is what an undo needs.
/// </para>
/// <para>
/// What it does not carry: other hard links to the file keep pointing at the old content, which is
/// then the held copy. Nothing here can tell, and nothing is lost by it.
/// </para>
/// </remarks>
public static class StagedReplace
{
    /// <summary>
    /// A working name beside <paramref name="path"/>, with the marker <em>before</em> the
    /// extension: <c>a.bertbrowser-rewrite-1234abcd.jpg</c> is still a picture to anything that
    /// goes by the name, and <c>a.jpg.bertbrowser-rewrite-1234abcd</c> is not.
    /// </summary>
    public static string Beside(string path, string marker)
    {
        var directory = Path.GetDirectoryName(path) ?? "";
        var stem = Path.GetFileNameWithoutExtension(path);
        var extension = Path.GetExtension(path);

        return Path.Combine(directory, stem + marker + Guid.NewGuid().ToString("N")[..8] + extension);
    }

    /// <summary>
    /// Puts <paramref name="replacement"/> where <paramref name="target"/> is and leaves what was
    /// there at <paramref name="backup"/>. On failure the target is still in place.
    /// </summary>
    public static void Swap(string replacement, string target, string backup)
    {
        try
        {
            File.Replace(replacement, target, backup, ignoreMetadataErrors: true);
        }
        catch (Exception)
        {
            // One of ReplaceFile's failures leaves the target already renamed to the backup and
            // nothing under its own name. Put it back before letting the failure out: a file that
            // has vanished because the second half of a swap failed is the outcome this must not have.
            try
            {
                if (!File.Exists(target) && File.Exists(backup)) File.Move(backup, target);
            }
            catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
            {
                // The caller's message names the backup.
            }

            throw;
        }
    }

    /// <summary>Whether a file name carries one of this app's working markers.</summary>
    public static bool IsMarked(string path, string marker) =>
        Path.GetFileName(path).Contains(marker, StringComparison.Ordinal);
}
