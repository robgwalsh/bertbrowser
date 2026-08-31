using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// Works out what deleting a selection would take with it, without touching anything. Every rule
/// that stops a delete reaching further than the user meant lives here, so the confirmation the
/// user answers is the same set of items the executor will act on.
/// </summary>
/// <remarks>
/// A selection can name a folder and something inside it at once — a flattened search result shows
/// both — and deleting both would leave the second one referring to a path that no longer exists.
/// The inner item is dropped as a benign no-op rather than reported: it really is being deleted,
/// just as part of its ancestor.
/// </remarks>
public sealed class DeletePlanner
{
    private readonly IDeleteProbe _probe;
    private readonly IRecycleProbe _recycleProbe;
    private readonly IReadOnlyCollection<string> _protectedKeys;

    /// <param name="probe">How the planner asks what is on disk.</param>
    /// <param name="protectedPaths">Locations to refuse outright; defaults to
    /// <see cref="ProtectedLocations.Default"/>. Injectable so the rule can be tested without
    /// depending on where Windows happens to be installed.</param>
    /// <param name="recycleProbe">How the planner asks whether a volume has a Recycle Bin that will
    /// take something. Defaults to "none has", so Core on its own routes everything to the holding
    /// folder rather than assuming a bin that may not be there.</param>
    public DeletePlanner(
        IDeleteProbe probe,
        IEnumerable<string>? protectedPaths = null,
        IRecycleProbe? recycleProbe = null)
    {
        _probe = probe;
        _recycleProbe = recycleProbe ?? NoRecycleProbe.Instance;
        _protectedKeys = protectedPaths is null
            ? ProtectedLocations.Default
            : ProtectedLocations.KeysOf(protectedPaths);
    }

    public DeletePlanner() : this(new FileSystemDeleteProbe())
    {
    }

    public DeletePlan Plan(IReadOnlyList<DeleteSource> sources, DeleteMode mode)
    {
        var distinct = Distinct(sources);
        if (distinct.Count == 0) return DeletePlan.Empty(mode);

        var deletions = new List<PlannedDelete>();
        var rejected = new List<RejectedDelete>();

        // Selected folders, so an item that is already going with its ancestor can be recognized.
        var directoryKeys = distinct
            .Where(d => _probe.DirectoryExists(d.Path))
            .Select(d => d.Key)
            .ToHashSet(StringComparer.Ordinal);

        foreach (var (path, key) in distinct)
        {
            var isDirectory = _probe.DirectoryExists(path);
            if (!isDirectory && !_probe.FileExists(path))
            {
                // Something inside an archive is refused by the same test — it has no path on
                // disk — but "no longer exists" would be a lie about a file the user can see. The
                // menu already hides Delete there; this is the backstop and it should say why.
                rejected.Add(new RejectedDelete(path, DeleteRejection.SourceMissing,
                    Archives.ArchivePath.Parse(path, File.Exists) is not null
                        ? $"'{Path.GetFileName(path)}' is inside an archive. Extract it first."
                        : $"'{Path.GetFileName(path)}' no longer exists."));
                continue;
            }

            if (ParentOf(path) is null)
            {
                rejected.Add(new RejectedDelete(path, DeleteRejection.SourceIsRoot,
                    $"'{path}' is a drive root and cannot be deleted."));
                continue;
            }

            if (_protectedKeys.Contains(key))
            {
                rejected.Add(new RejectedDelete(path, DeleteRejection.ProtectedLocation,
                    $"'{path}' is a system location and will not be deleted."));
                continue;
            }

            if (ProtectedLocations.IsInsideRecycleBin(path))
            {
                rejected.Add(new RejectedDelete(path, DeleteRejection.ProtectedLocation,
                    $"'{Path.GetFileName(path)}' is in the Recycle Bin; empty it from Windows instead."));
                continue;
            }

            if (directoryKeys.Any(other => other != key && PathKey.IsUnder(key, other)))
            {
                rejected.Add(new RejectedDelete(path, DeleteRejection.InsideADeletedFolder,
                    $"'{Path.GetFileName(path)}' is inside a folder that is being deleted too."));
                continue;
            }

            deletions.Add(new PlannedDelete(path, isDirectory, DispositionFor(mode, path)));
        }

        return new DeletePlan(mode, deletions, rejected);
    }

    /// <summary>
    /// Where one item is really going. A Recycle Bin delete falls back to the holding folder when
    /// the item's volume has no bin — a network share, removable media with it turned off — because
    /// the alternative the shell offers is erasing the item outright, which is not what the user
    /// asked for and not something to discover afterwards.
    /// </summary>
    private DeleteDisposition DispositionFor(DeleteMode mode, string path) => mode switch
    {
        DeleteMode.Permanent => DeleteDisposition.Erase,
        DeleteMode.Staged => DeleteDisposition.Stage,
        _ => _recycleProbe.CanRecycle(path) ? DeleteDisposition.Recycle : DeleteDisposition.Stage,
    };

    /// <summary>Drops repeats, so nothing is planned — or counted in the confirmation — twice.</summary>
    private static List<(string Path, string Key)> Distinct(IReadOnlyList<DeleteSource> sources)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var result = new List<(string, string)>();
        foreach (var source in sources)
        {
            if (string.IsNullOrWhiteSpace(source.Path)) continue;
            string key;
            try
            {
                key = PathKey.Canonicalize(source.Path);
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                continue;
            }
            if (seen.Add(key)) result.Add((source.Path, key));
        }
        return result;
    }

    private static string? ParentOf(string path)
    {
        var parent = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)));
        return string.IsNullOrEmpty(parent) ? null : parent;
    }
}
