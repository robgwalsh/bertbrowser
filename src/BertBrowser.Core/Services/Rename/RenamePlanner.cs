using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Rename;

/// <summary>The filesystem questions <see cref="RenamePlanner"/> asks. Abstracted so the collision
/// rules can be unit-tested against layouts that would otherwise need real files on a real disk.</summary>
public interface IRenameProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);
}

/// <summary>Real-filesystem <see cref="IRenameProbe"/>.</summary>
public sealed class FileSystemRenameProbe : IRenameProbe
{
    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);
}

/// <summary>
/// Works out what renaming a selection to a pattern would produce, without touching anything. Every
/// rule that stops a rename destroying something lives here: nothing is ever renamed onto a name
/// that is already taken, and two items can never be planned onto the same name.
/// </summary>
/// <remarks>
/// A name held by another item <em>in the same batch</em> is not a collision — that item is about to
/// vacate it, which is what makes rotating a set of names work — but only if that item is itself
/// being renamed. If it was rejected, it stays put, and whoever aimed at its name is rejected too.
/// </remarks>
public sealed class RenamePlanner
{
    private readonly IRenameProbe _probe;

    public RenamePlanner(IRenameProbe probe) => _probe = probe;

    public RenamePlanner() : this(new FileSystemRenameProbe())
    {
    }

    public RenamePlan Plan(IReadOnlyList<RenameSource> sources, string pattern)
    {
        var distinct = Distinct(sources);
        if (distinct.Count == 0) return RenamePlan.Empty;

        var names = RenamePattern.Apply(distinct.Select(d => d.Source).ToList(), pattern);

        var renames = new List<PlannedRename>();
        var rejected = new List<RejectedRename>();

        // Names spoken for by earlier entries in this same plan, so two items can never be planned
        // onto one name however the pattern was written.
        var claimed = new HashSet<string>(StringComparer.Ordinal);
        var sourceKeys = distinct.Select(d => d.Key).ToHashSet(StringComparer.Ordinal);
        // The batch members that are really on disk, and so really in the way until they move.
        var occupied = new HashSet<string>(StringComparer.Ordinal);
        // Selected folders, so an item that would have the ground moved out from under it can be
        // recognized before anything is planned.
        var directoryKeys = distinct
            .Where(d => _probe.DirectoryExists(d.Source.Path))
            .Select(d => d.Key)
            .ToHashSet(StringComparer.Ordinal);

        for (var i = 0; i < distinct.Count; i++)
        {
            var (source, key) = distinct[i];
            var name = names[i];
            var path = source.Path;

            var isDirectory = _probe.DirectoryExists(path);
            if (!isDirectory && !_probe.FileExists(path))
            {
                rejected.Add(new RejectedRename(path, RenameRejection.SourceMissing,
                    $"'{Path.GetFileName(path)}' no longer exists."));
                continue;
            }
            occupied.Add(key);

            if (ParentOf(path) is not { } parent)
            {
                rejected.Add(new RejectedRename(path, RenameRejection.SourceIsRoot,
                    $"'{path}' is a drive root and cannot be renamed."));
                continue;
            }

            if (directoryKeys.Any(other => other != key && PathKey.IsUnder(key, other)))
            {
                rejected.Add(new RejectedRename(path, RenameRejection.InsideARenamedFolder,
                    $"'{Path.GetFileName(path)}' is inside a folder that is being renamed too."));
                continue;
            }

            if (RenamePattern.Validate(name) is { } problem)
            {
                rejected.Add(new RejectedRename(path, RenameRejection.InvalidName, problem));
                continue;
            }

            var target = Path.Combine(parent, name);
            var targetKey = PathKey.Canonicalize(target);

            if (!claimed.Add(targetKey))
            {
                rejected.Add(new RejectedRename(path, RenameRejection.NameTaken,
                    $"Two of the selected items would both be named '{name}'."));
                continue;
            }

            // Occupied by something outside the batch is a hard stop; occupied by something inside
            // it is provisionally fine — the second pass below confirms that item really moves.
            if (targetKey != key && !sourceKeys.Contains(targetKey) && Exists(target))
            {
                rejected.Add(new RejectedRename(path, RenameRejection.NameTaken,
                    $"'{name}' already exists in this folder."));
                continue;
            }

            renames.Add(new PlannedRename(path, target, isDirectory));
        }

        return new RenamePlan(ConfirmVacancies(renames, rejected, occupied), rejected);
    }

    /// <summary>Second pass: an item aimed at a name held by another selected item is only safe if
    /// that item is itself leaving. Dropping one item can therefore doom another that was counting
    /// on it to move, so this repeats until nothing more changes.</summary>
    private static List<PlannedRename> ConfirmVacancies(
        List<PlannedRename> renames, List<RejectedRename> rejected, HashSet<string> occupied)
    {
        while (true)
        {
            var vacating = renames
                .Where(r => !r.IsNoOp)
                .Select(r => PathKey.Canonicalize(r.SourcePath))
                .ToHashSet(StringComparer.Ordinal);

            var doomed = renames.FirstOrDefault(r =>
            {
                if (r.IsNoOp) return false;
                var targetKey = PathKey.Canonicalize(r.TargetPath);
                // Its own name, cased differently, is not something it has to wait for.
                if (targetKey == PathKey.Canonicalize(r.SourcePath)) return false;
                return occupied.Contains(targetKey) && !vacating.Contains(targetKey);
            });
            if (doomed is null) return renames;

            renames.Remove(doomed);
            rejected.Add(new RejectedRename(doomed.SourcePath, RenameRejection.NameTaken,
                $"'{doomed.TargetName}' already exists in this folder."));
        }
    }

    private bool Exists(string path) => _probe.DirectoryExists(path) || _probe.FileExists(path);

    /// <summary>Drops repeats — a selection can name the same item twice only by accident, and
    /// renaming it twice would number it twice.</summary>
    private static List<(RenameSource Source, string Key)> Distinct(IReadOnlyList<RenameSource> sources)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var result = new List<(RenameSource, string)>();
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
            if (seen.Add(key)) result.Add((source, key));
        }
        return result;
    }

    private static string? ParentOf(string path)
    {
        var parent = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)));
        return string.IsNullOrEmpty(parent) ? null : parent;
    }
}
