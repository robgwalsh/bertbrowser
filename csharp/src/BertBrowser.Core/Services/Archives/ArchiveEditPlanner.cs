using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Decides whether a container may be edited, and what the edit would amount to.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing is modified in place, ever.</b> No managed library can, and .NET's own
/// <c>ZipArchive</c> update mode does it by materialising every entry into memory and committing on
/// <c>Dispose</c> — a 4 GB zip is 4 GB of RAM, and a crash half-way leaves a corrupt archive where
/// the user's data was. So every edit is a full rewrite, and the honest thing is to say so before
/// starting rather than to look fast and take ten minutes.
/// </para>
/// <para>
/// <b>Most of this file is refusals, and that is the design.</b> The formats and shapes below
/// cannot be rewritten without silently changing or losing something, so each is refused by name.
/// Being told "7z archives cannot be edited here" is a far better outcome than a rewrite that
/// quietly turns a solid archive into a non-solid one twice the size.
/// </para>
/// </remarks>
public sealed class ArchiveEditPlanner
{
    /// <summary>
    /// Above this, an edit is refused with its cost rather than silently taking minutes.
    /// </summary>
    public const long MaxRewriteBytes = 2L * 1024 * 1024 * 1024;

    /// <summary>Formats whose rewrite this app can actually produce.</summary>
    private static bool CanRewrite(ArchiveFormat format) => format.Writable;

    public ArchiveEditPlan Plan(
        ArchiveIndex index, string archiveFile, long archiveBytes, IReadOnlyList<ArchiveEdit> edits)
    {
        if (!index.Ok)
        {
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Unreadable,
                index.Error ?? "The archive could not be read.");
        }

        if (ArchiveFormats.Match(Path.GetFileName(archiveFile)) is not { } format)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Unreadable, "That is not an archive this app can read.");

        // Worded for editing rather than reusing ArchiveWriteRules.WhyNotWritable, which is about
        // creating: "7z archives cannot be created" is a confusing thing to be told when you were
        // trying to delete a file out of one.
        if (!CanRewrite(format))
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.FormatNotWritable,
                $"{format.Suffix} archives can be read but not changed — nothing here can write one.");

        // A rewrite would have to recompress every block, and cannot reproduce the solid layout —
        // so the file that came back would be materially different from the one that went in.
        if (index.Capabilities.SequentialOnly && format.RandomAccess)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Solid,
                "This archive is solid. Changing it would have to rebuild it differently, so it is not offered.");

        // Re-encrypting means holding a password in order to *write* with it, which is a promise a
        // file browser should not make — and a rewrite that dropped the encryption would be worse.
        if (index.Capabilities.IsEncrypted)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Encrypted,
                "This archive is encrypted. Changing it would remove the encryption, so it is not offered.");

        if (!index.Capabilities.IsComplete)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.Incomplete,
                "This archive is incomplete — part of it is missing. Changing it would make that permanent.");

        if (archiveBytes > MaxRewriteBytes)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.TooLarge,
                $"Changing this archive would mean rewriting {ByteSizeFormatter.Format(archiveBytes)}, " +
                "which is more than this will do in one go.");

        if (edits.Count == 0)
            return ArchiveEditPlan.Refused(
                ArchiveEditRejection.NothingToDo, "There is nothing to change.");

        // --- what each edit means, against what is really in there ---

        var removals = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var renames = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var additions = new List<AddFile>();

        foreach (var edit in edits)
        {
            switch (edit)
            {
                case RemoveEntry remove:
                {
                    if (index.Find(remove.EntryPath) is not { } node)
                        return Missing(remove.EntryPath);

                    // A folder takes everything under it, which is what deleting a folder means
                    // everywhere else in this app.
                    foreach (var path in Subtree(node)) removals.Add(path);
                    break;
                }

                case RenameEntry rename:
                {
                    if (index.Find(rename.EntryPath) is not { } node)
                        return Missing(rename.EntryPath);

                    if (RenamePattern.Validate(rename.NewName) is { } problem)
                        return ArchiveEditPlan.Refused(ArchiveEditRejection.InvalidName, problem);

                    var parent = Path.GetDirectoryName(rename.EntryPath) ?? "";
                    var target = parent.Length == 0
                        ? rename.NewName
                        : parent + "\\" + rename.NewName;

                    // Nothing may land on a name the container already holds. Unlike a filesystem
                    // rename there is no staging trick available here — the rewrite writes each
                    // entry once — so a swap is refused rather than half-done.
                    if (index.Find(target) is not null)
                        return ArchiveEditPlan.Refused(
                            ArchiveEditRejection.NameTaken,
                            $"'{rename.NewName}' is already in this archive.");

                    // Renaming a folder moves everything beneath it.
                    foreach (var path in Subtree(node))
                        renames[path] = target + path[rename.EntryPath.Length..];
                    break;
                }

                case AddFile add:
                {
                    if (index.Find(add.EntryPath) is not null)
                        return ArchiveEditPlan.Refused(
                            ArchiveEditRejection.NameTaken,
                            $"'{Path.GetFileName(add.EntryPath)}' is already in this archive.");

                    additions.Add(add);
                    break;
                }
            }
        }

        return new ArchiveEditPlan(
            archiveFile, edits, renames, removals, additions, archiveBytes, Rejected: null);
    }

    private static ArchiveEditPlan Missing(string entryPath) =>
        ArchiveEditPlan.Refused(
            ArchiveEditRejection.EntryMissing,
            $"'{Path.GetFileName(entryPath)}' is no longer in this archive.");

    /// <summary>An entry and, when it is a folder, everything under it.</summary>
    private static IEnumerable<string> Subtree(ArchiveNode node)
    {
        yield return node.Path;
        if (!node.IsDirectory) yield break;

        var stack = new Stack<ArchiveNode>(node.Children ?? []);
        while (stack.Count > 0)
        {
            var next = stack.Pop();
            yield return next.Path;
            foreach (var child in next.Children ?? []) stack.Push(child);
        }
    }
}
