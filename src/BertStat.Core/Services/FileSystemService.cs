using System.IO.Enumeration;
using BertStat.Core.Models;

namespace BertStat.Core.Services;

public interface IFileSystemService
{
    /// <summary>Enumerates the direct children of a directory. Throws on access denied.</summary>
    IReadOnlyList<FileEntry> ListDirectory(string path);

    /// <summary>True if the directory has at least one subdirectory (cheap probe for tree expanders).</summary>
    bool HasSubdirectories(string path);

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

    public bool HasSubdirectories(string path)
    {
        try
        {
            using var e = Directory.EnumerateDirectories(path).GetEnumerator();
            return e.MoveNext();
        }
        catch (UnauthorizedAccessException) { return false; }
        catch (IOException) { return false; }
    }

    public IReadOnlyList<DriveInfo> GetDrives() =>
        DriveInfo.GetDrives().Where(d => d.IsReady).ToList();
}
