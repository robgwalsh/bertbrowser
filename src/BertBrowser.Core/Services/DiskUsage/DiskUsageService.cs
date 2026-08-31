using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Core.Services.DiskUsage;

public interface IDiskUsageService
{
    /// <summary>
    /// The biggest files under <paramref name="rootPath"/>, or across every indexed volume when it
    /// is null, largest first.
    /// </summary>
    /// <remarks>
    /// Unscoped, this is a full scan of the index and takes seconds on a large disk. It is meant to
    /// be reached from an explicit "analyse this" gesture, never from typing, and the token is
    /// honoured so changing the root abandons the scan in flight.
    /// </remarks>
    Task<LargestFilesOutcome> LargestFilesAsync(
        string? rootPath, int limit, bool includeHidden, CancellationToken ct);

    /// <summary>
    /// What <paramref name="directory"/> is made of, by direct child, largest first.
    /// </summary>
    /// <remarks>
    /// The file half of the answer never needs the index — those sizes come from the enumeration
    /// itself — so a leaf folder is fully answerable on a volume nothing has ever indexed.
    /// </remarks>
    Task<DiskUsageBreakdown> BreakdownAsync(string directory, bool includeHidden, CancellationToken ct);
}

/// <summary>
/// The async facade over the disk-usage queries, shaped like <see cref="SearchService"/>: the
/// repositories below are synchronous ADO.NET, and this is what keeps ViewModels off them.
/// </summary>
/// <remarks>
/// Every number here comes from data the MFT pass already wrote — <c>fs_entry</c> for files and
/// <c>dir_size_cache</c> for folder totals. Nothing in this class walks the filesystem to size a
/// folder, which is the same rule the file list and the folder tree follow.
/// </remarks>
public sealed class DiskUsageService(
    FsIndexRepository index,
    DirSizeRepository dirSizes,
    IFileSystemService fileSystem,
    IMftIndexService mftIndex,
    IArchiveBrowser? archives = null) : IDiskUsageService
{
    /// <summary>
    /// The biggest entries under a folder inside a container, from the container's own index.
    /// </summary>
    /// <remarks>
    /// Always <see cref="DiskUsageAvailability.Ready"/>: an archive records every uncompressed
    /// length, so unlike a volume there is no "not indexed yet" state to be in. That is what makes
    /// pointing the treemap at a backup zip the one place this view is never approximate.
    /// </remarks>
    private LargestFilesOutcome LargestInArchive(
        ArchivePath root, int limit, CancellationToken ct)
    {
        var archive = archives!.ReadArchive(root.ArchiveFile);
        if (!archive.Ok) return new LargestFilesOutcome([], DiskUsageAvailability.NotIndexed);

        var start = archive.Find(root.EntryPath);
        if (start is null) return new LargestFilesOutcome([], DiskUsageAvailability.Ready);

        var files = new List<SearchHit>();
        var stack = new Stack<ArchiveNode>();
        stack.Push(start);

        while (stack.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            var node = stack.Pop();

            if (!node.IsDirectory)
            {
                files.Add(new SearchHit(
                    ArchivePath.Compose(root.ArchiveFile, node.Path),
                    RelativeDirDisplay: Path.GetDirectoryName(node.Path) ?? "",
                    node.Name,
                    IsDirectory: false,
                    node.SizeBytes,
                    node.Modified?.ToUniversalTime() ?? default));
                continue;
            }

            foreach (var child in node.Children ?? []) stack.Push(child);
        }

        files.Sort((a, b) => b.SizeBytes.CompareTo(a.SizeBytes));
        if (files.Count > limit) files.RemoveRange(limit, files.Count - limit);

        return new LargestFilesOutcome(files, DiskUsageAvailability.Ready);
    }

    public Task<LargestFilesOutcome> LargestFilesAsync(
        string? rootPath, int limit, bool includeHidden, CancellationToken ct) =>
        Task.Run(() =>
        {
            // fs_entry knows nothing about what is inside a container, and must never be taught:
            // its rows are PathKey-keyed, and a virtual key sorts strictly inside the archive's own
            // containing folder as well as inside the archive. Answered from the container's index
            // instead — where the sizes are exact and already in memory, so this is both correct
            // and free.
            if (rootPath is { Length: > 0 } && archives?.Resolve(rootPath) is { } inArchive)
                return LargestInArchive(inArchive, limit, ct);

            var rootKey = rootPath is { Length: > 0 } ? PathKey.Canonicalize(rootPath) : null;
            var files = index.LargestFiles(rootPath, limit, includeHidden);
            ct.ThrowIfCancellationRequested();

            var availability = DiskUsageRules.Classify(
                rootKey,
                files.Count,
                files.Count > 0 ? files[0].SizeBytes : 0,
                mftIndex.IsBuilding,
                rootKey is null ? mftIndex.AnyIndexed : mftIndex.IsIndexed(rootKey));

            // Rows that are all zero are not a result set, they are the absence of one. Handing
            // them back would leave every caller re-deciding whether to draw them.
            return availability == DiskUsageAvailability.NoSizeData
                ? new LargestFilesOutcome([], availability)
                : new LargestFilesOutcome(files, availability);
        }, ct);

    public Task<DiskUsageBreakdown> BreakdownAsync(
        string directory, bool includeHidden, CancellationToken ct) =>
        Task.Run(() =>
        {
            var rootKey = PathKey.Canonicalize(directory);
            var rootDisplay = PathKey.NormalizeDisplay(directory);

            var entries = fileSystem.ListDirectory(directory);
            ct.ThrowIfCancellationRequested();

            var visible = includeHidden
                ? entries
                : entries.Where(e => !e.Attributes.HasFlag(FileAttributes.Hidden)).ToList();

            // One batched lookup for every subfolder's total, exactly as the file list and the
            // folder tree hydrate their size columns. Files need no lookup: the enumeration
            // already carried their real lengths.
            // Inside an archive there is nothing to look up: the lister already carried every
            // folder's exact recursive total, because the numbers were in the container's own
            // directory. Asking dir_size_cache would be a round trip that can only miss — and a
            // miss reads as "unknown", which would draw a treemap of files with every folder
            // greyed out and then have ClassifyBreakdown blame the index for it.
            var insideArchive = ArchivePath.LooksVirtual(directory) && !Directory.Exists(directory);

            var dirKeys = insideArchive
                ? []
                : visible.Where(e => e.IsDirectory)
                    .Select(e => PathKey.Canonicalize(e.FullPath))
                    .ToList();
            var cached = dirKeys.Count > 0
                ? dirSizes.GetMany(dirKeys)
                : new Dictionary<string, DirSizeResult>(StringComparer.Ordinal);
            ct.ThrowIfCancellationRequested();

            var children = new List<DiskUsageNode>(visible.Count);
            foreach (var entry in visible)
            {
                var key = PathKey.Canonicalize(entry.FullPath);
                long? size;
                var incomplete = false;
                if (entry.IsDirectory && insideArchive)
                {
                    size = entry.SizeBytes;
                }
                else if (entry.IsDirectory)
                {
                    // A missing row is unknown, and stays null all the way to the view.
                    if (cached.TryGetValue(key, out var row))
                    {
                        size = row.SizeBytes;
                        incomplete = row.Incomplete;
                    }
                    else
                    {
                        size = null;
                    }
                }
                else
                {
                    size = entry.SizeBytes;
                }

                children.Add(new DiskUsageNode(
                    key,
                    entry.FullPath,
                    entry.Name,
                    entry.IsDirectory,
                    size,
                    incomplete,
                    entry.Attributes.HasFlag(FileAttributes.Hidden)));
            }

            // Largest first, unknowns last — they have no area to give and belong out of the way.
            children.Sort((a, b) => (b.SizeBytes ?? -1).CompareTo(a.SizeBytes ?? -1));

            var total = insideArchive
                ? children.Sum(c => c.SizeBytes ?? 0)
                : dirSizes.Get(directory)?.SizeBytes;
            var unknownCount = children.Count(c => c.SizeBytes is null);

            // Ready is passed directly rather than through ClassifyBreakdown, and the distinction
            // is the same one that makes ClassifyBreakdown a separate function from Classify: it
            // weighs evidence *about the index*, and inside a container there is no index to have
            // evidence about. Asking it would be the same category error, one level down — and it
            // would answer NoSizeData about numbers that are exact.
            var availability = insideArchive
                ? DiskUsageAvailability.Ready
                : DiskUsageRules.ClassifyBreakdown(
                    dirKeys.Count,
                    children.Count(c => c.IsDirectory && c.SizeBytes is not null),
                    mftIndex.IsBuilding,
                    mftIndex.IsIndexed(rootKey));

            return new DiskUsageBreakdown(
                rootDisplay,
                total,
                children,
                DiskUsageRules.Unaccounted(total, children),
                unknownCount,
                availability);
        }, ct);
}
