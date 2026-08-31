using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Archives;

/// <summary>The filesystem questions <see cref="ExtractPlanner"/> asks.</summary>
/// <remarks>
/// Abstracted for the reason <see cref="NewItem.INewItemProbe"/> is: the rules about where an
/// extract may write are worth testing without a real disk under them.
/// </remarks>
public interface IExtractProbe
{
    bool DirectoryExists(string path);
    bool FileExists(string path);
}

/// <summary>Real-filesystem <see cref="IExtractProbe"/>.</summary>
public sealed class FileSystemExtractProbe : IExtractProbe
{
    public bool DirectoryExists(string path) => Directory.Exists(path);
    public bool FileExists(string path) => File.Exists(path);
}

/// <summary>
/// Decides what an extract would write, touching nothing.
/// </summary>
/// <remarks>
/// <para>
/// The plan is built while a dialog sits open, so <see cref="ExtractExecutor"/> re-applies every
/// rule against live disk before writing — the same contract the transfer, rename, delete and
/// new-item executors all keep.
/// </para>
/// <para>
/// <b>The byte total is exact and free for an addressable container</b>, because the uncompressed
/// lengths are already in its directory — better than the filesystem case, where a folder's size is
/// a cache lookup that may miss. A sequential one reports <c>BytesAreExact: false</c>: pulling three
/// files out of a solid archive still reads the whole block, and a bar that promised four megabytes
/// and then read two gigabytes is worse than one that never claimed to know.
/// </para>
/// </remarks>
public sealed class ExtractPlanner
{
    private readonly IExtractProbe _probe;

    public ExtractPlanner(IExtractProbe probe) => _probe = probe;
    public ExtractPlanner() : this(new FileSystemExtractProbe()) { }

    /// <summary>
    /// Plans pulling <paramref name="entryPaths"/> (or everything under
    /// <paramref name="relativeTo"/> when empty) out of <paramref name="index"/>.
    /// </summary>
    public ExtractPlan Plan(
        ArchiveIndex index,
        string archiveFile,
        string relativeTo,
        IReadOnlyList<string> entryPaths,
        string destinationDirectory,
        ExtractConflict conflict)
    {
        if (!index.Ok)
        {
            return ExtractPlan.Refused(
                index.Failure == ArchiveFailure.PasswordRequired
                    ? ExtractRejection.PasswordRequired
                    : ExtractRejection.ArchiveUnreadable,
                index.Error ?? "The archive could not be read.");
        }

        if (ExtractRules.RejectDestination(destinationDirectory, _probe.DirectoryExists, _probe.FileExists)
            is { } bad)
        {
            return ExtractPlan.Refused(bad, bad switch
            {
                ExtractRejection.DestinationNotDirectory =>
                    "The destination is a file, not a folder.",
                ExtractRejection.DestinationInsideArchive =>
                    "An archive cannot be extracted into itself.",
                _ => "The destination folder is missing.",
            });
        }

        // Nothing named means "everything under here", which is what the toolbar and an
        // empty-space right-click mean by Extract.
        var roots = entryPaths.Count > 0
            ? entryPaths
            : (index.Children(relativeTo) ?? []).Select(n => n.Path).ToList();

        var files = new List<ArchiveNode>();
        var directories = new List<ArchiveNode>();
        foreach (var root in roots)
        {
            if (index.Find(root) is not { } node) continue;
            Collect(node, files, directories);
        }

        if (files.Count == 0 && directories.Count == 0)
        {
            return ExtractPlan.Refused(
                ExtractRejection.NothingToExtract, "There is nothing here to extract.");
        }

        var items = new List<PlannedExtraction>(files.Count + directories.Count);
        var conflicts = new List<string>();
        long total = 0;

        // Directories first and in depth order, so the executor can create them before the files
        // that land in them — and so a cancel can remove them in reverse and find them empty.
        foreach (var dir in directories.OrderBy(d => d.Path.Length))
        {
            items.Add(new PlannedExtraction(
                dir.Path,
                ExtractRules.TargetFor(dir.Path, relativeTo, destinationDirectory),
                0, IsDirectory: true));
        }

        foreach (var file in files)
        {
            var target = ExtractRules.TargetFor(file.Path, relativeTo, destinationDirectory);

            if (Exists(target))
            {
                conflicts.Add(target);
                if (conflict == ExtractConflict.Skip) continue;

                target = UniquePath.For(
                    target, isDirectory: false, _probe.DirectoryExists, _probe.FileExists);
            }

            items.Add(new PlannedExtraction(file.Path, target, file.SizeBytes, IsDirectory: false));
            total += file.SizeBytes;
        }

        return new ExtractPlan(
            archiveFile, destinationDirectory, items, conflicts, Rejected: null,
            TotalBytes: total,
            BytesAreExact: !index.Capabilities.SequentialOnly);
    }

    private static void Collect(ArchiveNode node, List<ArchiveNode> files, List<ArchiveNode> directories)
    {
        if (!node.IsDirectory)
        {
            // A symlink entry is listed but never recreated: writing one would put a link into the
            // destination pointing wherever the container asked, which is the escape the index's
            // own key rules exist to prevent.
            if (node.LinkTarget is null) files.Add(node);
            return;
        }

        directories.Add(node);
        foreach (var child in node.Children ?? []) Collect(child, files, directories);
    }

    private bool Exists(string path) => _probe.FileExists(path) || _probe.DirectoryExists(path);
}
