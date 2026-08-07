using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// Decides what a drop would do, without touching anything. Every rule that protects data lives
/// here so it can be tested in isolation: a folder is never allowed into its own subtree, sources
/// that travel inside another source are dropped, and roots are refused.
/// </summary>
/// <remarks>
/// Containment is checked twice — once on the literal paths and once on link-resolved paths —
/// because a junction can make an unrelated-looking destination physically sit inside the source.
/// Both checks must pass, so an unreadable or unresolvable link fails closed onto the literal one.
/// </remarks>
public sealed class TransferPlanner
{
    private readonly ITransferProbe _probe;

    public TransferPlanner(ITransferProbe probe) => _probe = probe;

    public TransferPlanner() : this(new FileSystemTransferProbe())
    {
    }

    public TransferPlan Plan(IReadOnlyList<string> sourcePaths, string destinationDirectory, TransferVerb verb)
    {
        if (string.IsNullOrWhiteSpace(destinationDirectory))
            return TransferPlan.Empty(verb, destinationDirectory ?? "");

        var transfers = new List<PlannedTransfer>();
        var rejected = new List<RejectedTransfer>();

        // A destination problem sinks every source, so report it once per source and stop.
        if (_probe.FileExists(destinationDirectory) && !_probe.DirectoryExists(destinationDirectory))
            return RejectAll(sourcePaths, verb, destinationDirectory, TransferRejection.DestinationNotDirectory,
                "The drop target is a file, not a folder.");
        if (!_probe.DirectoryExists(destinationDirectory))
            return RejectAll(sourcePaths, verb, destinationDirectory, TransferRejection.DestinationMissing,
                "The destination folder no longer exists.");

        var destKey = Key(destinationDirectory);
        var destResolvedKey = Key(_probe.ResolveFinalPath(destinationDirectory));

        // Distinct sources, and the keys of the directories among them, so a source nested under
        // another can be recognized before anything is planned.
        var sources = Distinct(sourcePaths);
        var directoryKeys = sources
            .Where(s => _probe.DirectoryExists(s.Path))
            .Select(s => s.Key)
            .ToHashSet(StringComparer.Ordinal);

        // Reserve the names already spoken for by earlier transfers in this same plan, so two
        // sources with the same name don't both get reported as landing on the same path.
        var claimed = new HashSet<string>(StringComparer.Ordinal);

        foreach (var (path, key) in sources)
        {
            var isDirectory = _probe.DirectoryExists(path);
            if (!isDirectory && !_probe.FileExists(path))
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.SourceMissing,
                    $"'{Path.GetFileName(path)}' no longer exists."));
                continue;
            }

            if (ParentOf(path) is not { } parent)
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.SourceIsRoot,
                    $"'{path}' is a drive root and cannot be moved."));
                continue;
            }

            if (directoryKeys.Any(other => other != key && PathKey.IsUnder(key, other)))
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.MovesWithAncestor,
                    $"'{Path.GetFileName(path)}' travels with the folder above it."));
                continue;
            }

            if (isDirectory && destKey == key)
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.DestinationIsSource,
                    $"Cannot drop '{Path.GetFileName(path)}' onto itself."));
                continue;
            }

            if (isDirectory && IsInside(path, key, destinationDirectory, destKey, destResolvedKey))
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.DestinationInsideSource,
                    $"Cannot move '{Path.GetFileName(path)}' into one of its own subfolders."));
                continue;
            }

            if (verb == TransferVerb.Move && Key(parent) == destKey)
            {
                rejected.Add(new RejectedTransfer(path, TransferRejection.AlreadyInDestination,
                    $"'{Path.GetFileName(path)}' is already here."));
                continue;
            }

            var destinationPath = Path.Combine(destinationDirectory, Path.GetFileName(path));
            var destinationKey = Key(destinationPath);
            var conflicts = !claimed.Add(destinationKey) || Exists(destinationPath);
            transfers.Add(new PlannedTransfer(path, isDirectory, destinationPath, conflicts));
        }

        return new TransferPlan(verb, destinationDirectory, transfers, rejected);
    }

    /// <summary>
    /// True when the destination sits at or under the source folder. Tested on the literal paths
    /// and again on link-resolved paths, so a junction cannot smuggle the destination inside the
    /// tree being moved.
    /// </summary>
    private bool IsInside(string sourcePath, string sourceKey, string destPath, string destKey, string destResolvedKey)
    {
        if (PathKey.IsUnder(destKey, sourceKey)) return true;

        var sourceResolvedKey = Key(_probe.ResolveFinalPath(sourcePath));
        if (sourceResolvedKey == destResolvedKey) return true;
        if (PathKey.IsUnder(destResolvedKey, sourceResolvedKey)) return true;

        // Mixed forms catch a link on exactly one side of the comparison.
        return PathKey.IsUnder(destResolvedKey, sourceKey) || PathKey.IsUnder(destKey, sourceResolvedKey);
    }

    private static TransferPlan RejectAll(
        IReadOnlyList<string> sourcePaths, TransferVerb verb, string destination,
        TransferRejection reason, string message) =>
        new(verb, destination, Array.Empty<PlannedTransfer>(),
            Distinct(sourcePaths).Select(s => new RejectedTransfer(s.Path, reason, message)).ToList());

    /// <summary>Distinct sources in input order, folded case-insensitively like the filesystem.</summary>
    private static List<(string Path, string Key)> Distinct(IReadOnlyList<string> sourcePaths)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var result = new List<(string, string)>();
        foreach (var path in sourcePaths)
        {
            if (string.IsNullOrWhiteSpace(path)) continue;
            string key;
            try
            {
                key = PathKey.Canonicalize(path);
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                continue; // unusable path: nothing sensible to plan
            }
            if (seen.Add(key)) result.Add((path, key));
        }
        return result;
    }

    /// <summary>Null for a drive/volume root, which has no parent to move out of.</summary>
    private static string? ParentOf(string path)
    {
        try
        {
            return Path.GetDirectoryName(Path.GetFullPath(path));
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return null;
        }
    }

    private bool Exists(string path) => _probe.DirectoryExists(path) || _probe.FileExists(path);

    private static string Key(string path)
    {
        try
        {
            return PathKey.Canonicalize(path);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return path.ToUpperInvariant();
        }
    }
}
