using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// The pure decisions an extract needs: where "here" is, and what one entry's destination path is.
/// </summary>
public static class ExtractRules
{
    /// <summary>
    /// Where "Extract here" puts things.
    /// </summary>
    /// <remarks>
    /// <b>Tarbomb avoidance, the right way round.</b> An archive whose contents sit under a single
    /// top-level folder already carries its own wrapper, and putting that inside a second folder
    /// named after the file gives you <c>project\project\src</c>. One that does not — the tarbomb —
    /// would otherwise spray its files across the folder you were browsing, so that one gets a
    /// wrapper named after the archive, deduplicated by <see cref="UniquePath"/>.
    /// </remarks>
    public static string DestinationFor(
        ArchiveIndex index,
        string archiveFile,
        Func<string, bool> directoryExists,
        Func<string, bool> fileExists)
    {
        var beside = Path.GetDirectoryName(archiveFile) ?? archiveFile;
        var top = index.Children("") ?? [];

        if (top.Count == 1 && top[0].IsDirectory) return beside;

        return UniquePath.For(
            Path.Combine(beside, StemOf(archiveFile)),
            isDirectory: true, directoryExists, fileExists);
    }

    /// <summary>
    /// The archive's name without its suffix — <c>backup.tar.gz</c> gives <c>backup</c>, not
    /// <c>backup.tar</c>, because the compound suffix is one thing and half of it is not a name.
    /// </summary>
    public static string StemOf(string archiveFile)
    {
        var name = Path.GetFileName(archiveFile);
        if (ArchiveFormats.Match(name) is { } format)
            return name[..^format.Suffix.Length];
        return Path.GetFileNameWithoutExtension(name);
    }

    /// <summary>
    /// Where one entry lands, given the subtree being extracted from.
    /// </summary>
    /// <param name="entryPath">Normalised entry key.</param>
    /// <param name="relativeTo">
    /// The folder inside the archive the extract is rooted at, so extracting <c>src</c> from inside
    /// it produces <c>dest\lib\util.js</c> rather than <c>dest\src\lib\util.js</c>. Empty for the
    /// archive's own root.
    /// </param>
    public static string TargetFor(string entryPath, string relativeTo, string destinationDirectory)
    {
        var relative = Relative(entryPath, relativeTo);
        return Path.Combine(destinationDirectory, relative);
    }

    internal static string Relative(string entryPath, string relativeTo)
    {
        if (relativeTo.Length == 0) return entryPath;

        var prefix = relativeTo.TrimEnd('\\') + "\\";
        return entryPath.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? entryPath[prefix.Length..]
            : Path.GetFileName(entryPath);
    }

    /// <summary>
    /// Whether a destination is safe to extract into: a real folder, and not inside the container
    /// being read from.
    /// </summary>
    /// <remarks>
    /// The second half is the interesting one. A virtual path canonicalizes happily, so
    /// "extract this zip into itself" produces a destination no executor could ever write to —
    /// better refused by name than failed one file at a time.
    /// </remarks>
    public static ExtractRejection? RejectDestination(
        string destinationDirectory,
        Func<string, bool> directoryExists,
        Func<string, bool> fileExists)
    {
        if (string.IsNullOrWhiteSpace(destinationDirectory))
            return ExtractRejection.DestinationMissing;

        if (ArchivePath.LooksVirtual(destinationDirectory) && !directoryExists(destinationDirectory))
            return ExtractRejection.DestinationInsideArchive;

        if (fileExists(destinationDirectory)) return ExtractRejection.DestinationNotDirectory;

        return null;
    }
}
