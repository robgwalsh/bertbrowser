using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Archives;

namespace BertBrowser.Core.Services.Compare;

/// <summary>The filesystem questions a comparison has to ask before it starts, injected so the
/// containment rules can be tested against link layouts that are impractical to create.</summary>
public interface ICompareProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);

    /// <summary>The physical path with every junction and symlink resolved, best effort.</summary>
    string ResolveFinalPath(string path);
}

/// <summary>
/// Whether two folders may be compared at all. Pure but for the probe, because every one of these
/// is a data-safety rule rather than a convenience: a comparison that runs on the wrong pair is
/// what makes "delete what is only on the right" delete the wrong thing.
/// </summary>
public static class CompareRefusal
{
    /// <summary>The reason to refuse, or null to go ahead.</summary>
    public static string? Check(string leftPath, string rightPath, ICompareProbe probe)
    {
        if (string.IsNullOrWhiteSpace(leftPath) || string.IsNullOrWhiteSpace(rightPath))
            return "Compare needs a folder open on each side.";

        // A path inside an archive is a real Windows path syntactically, so it canonicalizes
        // happily and lands inside PrefixBounds of the container's own folder — after which a
        // subtree query over that folder returns archive interiors. Refused outright, as search,
        // bookmarks and disk usage each refuse it.
        if (ArchivePath.Parse(leftPath, probe.FileExists) is not null ||
            ArchivePath.Parse(rightPath, probe.FileExists) is not null)
            return "Comparing inside an archive is not supported. Extract it first.";

        if (!probe.DirectoryExists(leftPath) || !probe.DirectoryExists(rightPath))
            return "Compare needs a folder on both sides.";

        // Checked literally and again on resolved paths, the double check TransferPlanner makes,
        // and for the same reason: a junction can put one of these inside the other without either
        // path saying so. Comparing a folder with its own subtree would have every file show up
        // twice over and "only on the right" would name the tree itself.
        if (Overlaps(leftPath, rightPath) ||
            Overlaps(probe.ResolveFinalPath(leftPath), probe.ResolveFinalPath(rightPath)))
            return "Compare needs two different folders, neither inside the other.";

        return null;
    }

    private static bool Overlaps(string a, string b)
    {
        var left = PathKey.Canonicalize(a);
        var right = PathKey.Canonicalize(b);
        return string.Equals(left, right, StringComparison.Ordinal)
            || PathKey.IsUnder(left, right)
            || PathKey.IsUnder(right, left);
    }
}

/// <summary>Real-filesystem <see cref="ICompareProbe"/>. Resolution is delegated to the transfer
/// probe rather than written twice — one definition of "where does this junction actually go".</summary>
public sealed class FileSystemCompareProbe : ICompareProbe
{
    private readonly Transfer.FileSystemTransferProbe _paths = new();

    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);

    public string ResolveFinalPath(string path) => _paths.ResolveFinalPath(path);
}
