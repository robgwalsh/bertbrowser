using System.IO.Enumeration;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// One immediate child of a directory, as the enumeration that found it already described it.
/// </summary>
/// <param name="IsReparsePoint">A junction or symbolic link. Carried because it is the one
/// attribute that decides whether a merge may descend, and re-reading it per entry afterwards
/// would be a stat the enumeration already paid for.</param>
/// <param name="SizeBytes">Bytes for a file; 0 for a directory, whose size is never read from
/// here — <see cref="ITransferSizeSource"/> answers that from the index instead.</param>
public readonly record struct TransferEntry(
    string Name,
    string FullPath,
    bool IsDirectory,
    bool IsReparsePoint,
    long SizeBytes,
    DateTime ModifiedUtc);

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

/// <summary>
/// A probe that can also list a directory's immediate children.
/// </summary>
/// <remarks>
/// <b>Separate from <see cref="ITransferProbe"/> on purpose.</b> <see cref="TransferPlanner"/> keeps
/// taking the narrower interface and therefore <em>cannot</em> enumerate — which is the invariant
/// that matters, because <c>DropPipeline.IsAllowed</c> builds a plan on every drag-over and a walk
/// there would hit the disk on hover. Making that impossible by type beats writing it down and
/// hoping. <see cref="TransferMergeExpander"/> takes this one, and runs only at drop time.
/// </remarks>
public interface ITransferEntrySource : ITransferProbe
{
    /// <summary>
    /// The directory's immediate children — <b>one level, never recursive</b>. The whole point of
    /// the expander is that it descends only where both sides have a directory, so a recursive
    /// listing here would be the one thing that must not happen. Empty when the directory cannot be
    /// read: an unreadable folder is not a merge opportunity, and a plan that throws part-way
    /// through building is worse than one that declines to expand.
    /// </summary>
    IReadOnlyList<TransferEntry> Entries(string directory);
}

/// <summary>Real-filesystem <see cref="ITransferProbe"/>.</summary>
public sealed class FileSystemTransferProbe : ITransferEntrySource
{
    /// <summary>Bounds the resolution loop so a cyclic junction layout can't hang it.</summary>
    private const int MaxHops = 32;

    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);

    /// <summary>
    /// One level, from the find data the enumeration already holds — name, attributes, length and
    /// last-write all come free, so listing a folder costs no stat per entry. The same trick
    /// <see cref="FileSystemWalker"/> uses, without its descent.
    /// </summary>
    public IReadOnlyList<TransferEntry> Entries(string directory)
    {
        try
        {
            var enumerable = new FileSystemEnumerable<TransferEntry>(
                directory,
                (ref FileSystemEntry entry) =>
                {
                    var isDirectory = entry.IsDirectory;
                    return new TransferEntry(
                        entry.FileName.ToString(),
                        entry.ToFullPath(),
                        isDirectory,
                        (entry.Attributes & FileAttributes.ReparsePoint) != 0,
                        isDirectory ? 0 : entry.Length,
                        entry.LastWriteTimeUtc.UtcDateTime);
                },
                new EnumerationOptions
                {
                    IgnoreInaccessible = false,
                    AttributesToSkip = 0,
                    RecurseSubdirectories = false,
                });

            return [.. enumerable];
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
            or ArgumentException or NotSupportedException or System.Security.SecurityException)
        {
            return [];
        }
    }

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
