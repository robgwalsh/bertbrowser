using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Answers "what is already in here?" from a container's index instead of from disk.
/// </summary>
/// <remarks>
/// <para>
/// This is why <see cref="RenamePlanner"/> needed no changes to work inside an archive. Its probe
/// seam exists so the collision rules can be tested against a fake filesystem; a container is just
/// another filesystem those rules know nothing about, and they turn out to be exactly right for it.
/// The dialog therefore previews every keystroke with the same function the edit obeys — the
/// property that whole design exists to keep.
/// </para>
/// <para>
/// It answers about paths <em>inside one container</em> and falls through to the real disk for
/// anything else, so a planner holding one is not lying about the rest of the world.
/// </para>
/// </remarks>
public sealed class ArchiveRenameProbe : IRenameProbe
{
    private readonly ArchiveIndex _index;
    private readonly string _archiveFile;

    public ArchiveRenameProbe(ArchiveIndex index, string archiveFile)
    {
        _index = index;
        _archiveFile = archiveFile;
    }

    public bool DirectoryExists(string path) => Find(path) is { IsDirectory: true };

    public bool FileExists(string path) => Find(path) is { IsDirectory: false };

    private ArchiveNode? Find(string path)
    {
        if (ArchivePath.Parse(path, IsThisArchive) is not { } entry) return null;
        return entry.IsRoot ? _index.Root : _index.Find(entry.EntryPath);
    }

    /// <summary>
    /// The one file this probe treats as a container. Deliberately not <c>File.Exists</c>: a probe
    /// that reached the disk would answer about other archives it has never read.
    /// </summary>
    private bool IsThisArchive(string candidate) =>
        candidate.Equals(_archiveFile, StringComparison.OrdinalIgnoreCase);
}
