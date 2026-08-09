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
    private readonly IReadOnlyCollection<string> _protectedKeys;

    /// <param name="probe">How the planner asks what is on disk.</param>
    /// <param name="protectedPaths">Locations to refuse outright; defaults to
    /// <see cref="ProtectedLocations.Default"/>. Injectable so the rule can be tested without
    /// depending on where Windows happens to be installed.</param>
    public DeletePlanner(IDeleteProbe probe, IEnumerable<string>? protectedPaths = null)
    {
        _probe = probe;
        _protectedKeys = protectedPaths is null
            ? ProtectedLocations.Default
            : ProtectedLocations.KeysOf(protectedPaths);
    }

    public DeletePlanner() : this(new FileSystemDeleteProbe())
    {
    }

    public DeletePlan Plan(IReadOnlyList<DeleteSource> sources, bool permanent)
    {
        var distinct = Distinct(sources);
        if (distinct.Count == 0) return DeletePlan.Empty(permanent);

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
                rejected.Add(new RejectedDelete(path, DeleteRejection.SourceMissing,
                    $"'{Path.GetFileName(path)}' no longer exists."));
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

            if (directoryKeys.Any(other => other != key && PathKey.IsUnder(key, other)))
            {
                rejected.Add(new RejectedDelete(path, DeleteRejection.InsideADeletedFolder,
                    $"'{Path.GetFileName(path)}' is inside a folder that is being deleted too."));
                continue;
            }

            deletions.Add(new PlannedDelete(path, isDirectory));
        }

        return new DeletePlan(permanent, deletions, rejected);
    }

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
