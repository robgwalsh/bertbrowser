using System.IO.Enumeration;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services;

/// <summary>What a single subdirectory scan found: whether there is any subdirectory at all, and
/// whether there is one that isn't hidden. The folder tree needs both from one probe, because its
/// expander follows the "Show hidden items" setting and has to flip without re-reading disk.</summary>
public readonly record struct SubdirectoryPresence(bool Any, bool AnyVisible);

public interface IFileSystemService
{
    /// <summary>Enumerates the direct children of a directory. Throws on access denied.</summary>
    IReadOnlyList<FileEntry> ListDirectory(string path);

    /// <summary>Cheap probe for tree expanders: does this directory have subdirectories, hidden
    /// or otherwise. Stops at the first non-hidden one; only an all-hidden directory is scanned
    /// through. Inaccessible directories report nothing rather than throwing.</summary>
    SubdirectoryPresence ProbeSubdirectories(string path);

    IReadOnlyList<DriveInfo> GetDrives();
}

public sealed class FileSystemService : IFileSystemService
{
    public IReadOnlyList<FileEntry> ListDirectory(string path)
    {
        var entries = new List<FileEntry>();
        var enumerable = new FileSystemEnumerable<FileEntry>(
            path,
            (ref FileSystemEntry entry) => new FileEntry(
                entry.FileName.ToString(),
                entry.ToFullPath(),
                entry.IsDirectory,
                entry.IsDirectory ? -1 : entry.Length,
                entry.LastWriteTimeUtc.UtcDateTime,
                entry.Attributes),
            new EnumerationOptions
            {
                IgnoreInaccessible = false,
                AttributesToSkip = 0, // show hidden/system like a power tool should
                RecurseSubdirectories = false,
            });
        entries.AddRange(enumerable);
        return entries;
    }

    public SubdirectoryPresence ProbeSubdirectories(string path)
    {
        var any = false;
        try
        {
            // DirectoryInfo, not Directory.EnumerateDirectories: the attributes come pre-populated
            // from the scan, so the hidden test costs no extra stat per subdirectory.
            foreach (var dir in new DirectoryInfo(path).EnumerateDirectories())
            {
                any = true;
                if (!dir.Attributes.HasFlag(FileAttributes.Hidden))
                    return new SubdirectoryPresence(true, true);
            }
        }
        catch (UnauthorizedAccessException) { }
        catch (IOException) { }
        return new SubdirectoryPresence(any, false);
    }

    public IReadOnlyList<DriveInfo> GetDrives() =>
        DriveInfo.GetDrives().Where(d => d.IsReady).ToList();
}
