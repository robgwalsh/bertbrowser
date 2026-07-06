using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

public interface IFileTransferService
{
    /// <summary>
    /// Copies a file or directory into <paramref name="destinationDir"/>, generating a
    /// "name (2)"-style unique name on collision. Returns the destination path.
    /// </summary>
    string CopyInto(string sourcePath, string destinationDir);

    /// <summary>
    /// Moves a file or directory into <paramref name="destinationDir"/>. Returns the
    /// destination path, or <paramref name="sourcePath"/> unchanged when the move is a
    /// no-op (source already lives in the destination directory).
    /// </summary>
    string MoveInto(string sourcePath, string destinationDir);
}

public sealed class FileTransferService : IFileTransferService
{
    public string CopyInto(string sourcePath, string destinationDir)
    {
        var isDirectory = RequireSource(sourcePath);
        if (isDirectory)
            GuardNotIntoSelf(sourcePath, destinationDir);

        var destination = UniqueDestination(destinationDir, Path.GetFileName(sourcePath), isDirectory);
        if (isDirectory)
            CopyDirectory(new DirectoryInfo(sourcePath), destination);
        else
            File.Copy(sourcePath, destination);
        return destination;
    }

    public string MoveInto(string sourcePath, string destinationDir)
    {
        var isDirectory = RequireSource(sourcePath);

        var sourceParent = Path.GetDirectoryName(Path.GetFullPath(sourcePath))
            ?? throw new InvalidOperationException("Cannot move a drive root.");
        if (PathKey.Canonicalize(sourceParent) == PathKey.Canonicalize(destinationDir))
            return sourcePath;

        if (isDirectory)
            GuardNotIntoSelf(sourcePath, destinationDir);

        var destination = UniqueDestination(destinationDir, Path.GetFileName(sourcePath), isDirectory);
        if (isDirectory)
        {
            try
            {
                Directory.Move(sourcePath, destination);
            }
            catch (IOException) // Directory.Move cannot cross volumes
            {
                CopyDirectory(new DirectoryInfo(sourcePath), destination);
                Directory.Delete(sourcePath, recursive: true);
            }
        }
        else
        {
            File.Move(sourcePath, destination);
        }
        return destination;
    }

    /// <summary>Returns true when the source is a directory; throws when it does not exist.</summary>
    private static bool RequireSource(string sourcePath)
    {
        if (Directory.Exists(sourcePath)) return true;
        if (File.Exists(sourcePath)) return false;
        throw new FileNotFoundException($"Source not found: {sourcePath}", sourcePath);
    }

    private static void GuardNotIntoSelf(string sourceDir, string destinationDir)
    {
        var sourceKey = PathKey.Canonicalize(sourceDir);
        var destKey = PathKey.Canonicalize(destinationDir);
        if (destKey == sourceKey || PathKey.IsUnder(destKey, sourceKey))
            throw new InvalidOperationException(
                $"Cannot copy or move '{Path.GetFileName(sourceDir)}' into itself or one of its subfolders.");
    }

    private static string UniqueDestination(string destinationDir, string name, bool isDirectory)
    {
        var candidate = Path.Combine(destinationDir, name);
        if (!EntryExists(candidate)) return candidate;

        // Directories number the whole name; files number before the extension.
        var stem = isDirectory ? name : Path.GetFileNameWithoutExtension(name);
        var extension = isDirectory ? "" : Path.GetExtension(name);
        for (var i = 2; ; i++)
        {
            candidate = Path.Combine(destinationDir, $"{stem} ({i}){extension}");
            if (!EntryExists(candidate)) return candidate;
        }
    }

    private static bool EntryExists(string path) => File.Exists(path) || Directory.Exists(path);

    private static void CopyDirectory(DirectoryInfo source, string destination)
    {
        Directory.CreateDirectory(destination);
        foreach (var entry in source.EnumerateFileSystemInfos())
        {
            // Skip junctions/symlinks, like DirectorySizeService, to avoid cycles.
            if ((entry.Attributes & FileAttributes.ReparsePoint) != 0) continue;

            if (entry is DirectoryInfo dir)
                CopyDirectory(dir, Path.Combine(destination, dir.Name));
            else if (entry is FileInfo file)
                file.CopyTo(Path.Combine(destination, file.Name));
        }
    }
}
