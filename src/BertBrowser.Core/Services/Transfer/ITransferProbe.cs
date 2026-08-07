namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// The filesystem questions <see cref="TransferPlanner"/> needs answered. Abstracted so the
/// planner's containment rules — the ones that stop a folder being moved into itself — can be
/// unit-tested against link layouts that are impractical to create on a real disk.
/// </summary>
public interface ITransferProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);

    /// <summary>
    /// The physical path with every symlink/junction resolved, including ones part-way along the
    /// path (<c>C:\link\sub</c> where <c>link</c> is a junction). Returns the fully-qualified input
    /// when nothing resolves, or when resolution fails — callers must treat the result as a best
    /// effort and keep their literal-path checks as well.
    /// </summary>
    string ResolveFinalPath(string path);
}

/// <summary>Real-filesystem <see cref="ITransferProbe"/>.</summary>
public sealed class FileSystemTransferProbe : ITransferProbe
{
    /// <summary>Bounds the resolution loop so a cyclic junction layout can't hang it.</summary>
    private const int MaxHops = 32;

    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);

    public string ResolveFinalPath(string path)
    {
        string full;
        try
        {
            full = Path.GetFullPath(path);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return path;
        }

        // .NET resolves a link only when it is the final component, so walk up the path looking for
        // the deepest ancestor that is one, splice in its target, and go round again — a target may
        // itself sit behind another link.
        for (var hop = 0; hop < MaxHops; hop++)
        {
            if (!TryResolveOnce(full, out var resolved)) return full;
            full = resolved;
        }
        return full;
    }

    /// <summary>Replaces the deepest link on the path with its target. False when there is none.</summary>
    private static bool TryResolveOnce(string full, out string resolved)
    {
        resolved = full;
        var suffix = "";

        for (string? current = full; current is not null; current = Path.GetDirectoryName(current))
        {
            if (LinkTarget(current) is { } target &&
                !target.Equals(current, StringComparison.OrdinalIgnoreCase))
            {
                resolved = suffix.Length == 0 ? target : Path.Combine(target, suffix);
                return true;
            }

            var name = Path.GetFileName(current);
            if (name.Length == 0) return false; // reached the root
            suffix = suffix.Length == 0 ? name : Path.Combine(name, suffix);
        }
        return false;
    }

    private static string? LinkTarget(string path)
    {
        try
        {
            FileSystemInfo? info =
                Directory.Exists(path) ? new DirectoryInfo(path) :
                File.Exists(path) ? new FileInfo(path) :
                null;
            if (info is null || (info.Attributes & FileAttributes.ReparsePoint) == 0) return null;
            return info.ResolveLinkTarget(returnFinalTarget: true)?.FullName;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
            or ArgumentException or NotSupportedException or System.Security.SecurityException)
        {
            return null; // unreadable link: fall back to the literal path
        }
    }
}
