using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.Archives;

/// <summary>Where the session's archive passwords are kept, if anywhere.</summary>
/// <remarks>
/// A seam rather than a concrete class so the listing layer never holds secrets itself, and so a
/// harness run can supply passwords without a dialog. The implementation lives in the App and keeps
/// nothing on disk.
/// </remarks>
public interface IArchivePasswords
{
    /// <summary>The password for this archive, or null if none has been given.</summary>
    string? For(string archiveFile);
}

/// <summary>
/// A listing failed because the container needs a password.
/// </summary>
/// <remarks>
/// An <see cref="IOException"/> subclass on purpose: everything that already catches one keeps
/// working, and the one caller that needs to tell this case apart — so it can offer to unlock —
/// tests for the subclass. A second error channel would have had to be threaded through the whole
/// listing path for one message.
/// </remarks>
public sealed class ArchiveLockedException(string archiveFile, string message)
    : IOException(message)
{
    public string ArchiveFile { get; } = archiveFile;
}

/// <summary>Nobody has any passwords. The default, and what the harness gets unless told otherwise.</summary>
public sealed class NoArchivePasswords : IArchivePasswords
{
    public string? For(string archiveFile) => null;
}

/// <summary>
/// The questions about archives that the listing seam cannot express.
/// </summary>
/// <remarks>
/// Separate from <see cref="IFileSystemService"/> on purpose: that interface is about directories
/// and gains nothing by learning what an archive is, and the five things that call it must stay
/// unable to tell the difference. This is what navigation, the preview pane and the context-menu
/// guards ask instead — implemented by the same object, so the two views cannot disagree.
/// </remarks>
public interface IArchiveBrowser
{
    /// <summary>A real directory, or somewhere inside a readable archive. Opens the container, so
    /// it belongs on a worker thread.</summary>
    bool CanList(string path);

    /// <summary>Whether this path is inside an archive, and which one.</summary>
    ArchivePath? Resolve(string path);

    /// <summary>The container's index, cached.</summary>
    ArchiveIndex ReadArchive(string archiveFile);
}

/// <summary>
/// Makes an archive look like a folder to everything that lists directories.
/// </summary>
/// <remarks>
/// <para>
/// A decorator over the real <see cref="FileSystemService"/>, which is why the five callers of
/// <see cref="IFileSystemService"/> needed no changes at all: the file list, the merge diff, the
/// disk-usage breakdown and the folder tree all keep asking the same three questions.
/// </para>
/// <para>
/// <b>An ordinary path never reaches the archive code.</b> <see cref="ArchivePath.LooksVirtual"/> is
/// a pure segment scan, so browsing <c>C:\Users\Rob</c> costs one string walk and nothing else.
/// </para>
/// </remarks>
public sealed class ArchiveAwareFileSystemService : IFileSystemService, IArchiveBrowser
{
    private readonly IFileSystemService _inner;
    private readonly IArchiveReader _reader;
    private readonly IArchivePasswords _passwords;
    private readonly ArchiveCache _cache;

    public ArchiveAwareFileSystemService(
        IFileSystemService inner,
        IArchiveReader reader,
        IArchivePasswords? passwords = null,
        ArchiveCache? cache = null)
    {
        _inner = inner;
        _reader = reader;
        _passwords = passwords ?? new NoArchivePasswords();
        _cache = cache ?? new ArchiveCache();
    }

    /// <summary>
    /// Reads a container, cached. Public because navigation and the preview pane both need the
    /// same index the listing was built from, and re-reading it would be both slower and — if the
    /// file changed in between — a different answer.
    /// </summary>
    public ArchiveIndex ReadArchive(string archiveFile) =>
        _cache.Get(archiveFile, _passwords.For(archiveFile),
            (file, password) => _reader.Read(file, password));

    /// <summary>Whether this path is inside an archive, and which one.</summary>
    public ArchivePath? Resolve(string path) => ArchivePath.Parse(path, IsArchiveFile);

    /// <summary>
    /// A path that can be listed: a real directory, or somewhere inside a readable archive.
    /// </summary>
    /// <remarks>
    /// This is what the navigation gate asks <em>after</em> the cheap syntactic check says maybe.
    /// It opens the container, so it belongs on a worker thread and nowhere near the UI one.
    /// </remarks>
    public bool CanList(string path)
    {
        if (Directory.Exists(path)) return true;
        if (Resolve(path) is not { } archive) return false;

        var index = ReadArchive(archive.ArchiveFile);
        // A failure is still navigable: the list shows the banner, which is the whole point of
        // letting the load rather than the gate decide. Only "there is no such folder in a
        // perfectly readable archive" is a refusal.
        return !index.Ok || index.Children(archive.EntryPath) is not null;
    }

    public IReadOnlyList<FileEntry> ListDirectory(string path)
    {
        if (Resolve(path) is not { } archive) return _inner.ListDirectory(path);

        var index = ReadArchive(archive.ArchiveFile);

        // The interface's contract is that listing throws on failure, and FileListViewModel already
        // turns an IOException into its error banner. Reusing that path means no new error plumbing
        // — and the one failure the caller must be able to tell apart gets a subclass rather than a
        // second channel, so anything catching IOException keeps working unchanged.
        if (index.Failure == ArchiveFailure.PasswordRequired)
            throw new ArchiveLockedException(
                archive.ArchiveFile, index.Error ?? "This archive is protected.");

        if (!index.Ok) throw new IOException(index.Error ?? "The archive could not be read.");

        var children = index.Children(archive.EntryPath)
            ?? throw new DirectoryNotFoundException(
                $"There is no folder named '{archive.EntryPath}' in this archive.");

        var rows = new List<FileEntry>(children.Count);
        foreach (var child in children)
            rows.Add(ToEntry(archive.ArchiveFile, child));
        return rows;
    }

    public SubdirectoryPresence ProbeSubdirectories(string path)
    {
        // This answers for an archive file as readily as for a folder inside one, and stays honest
        // rather than being made to lie in order to keep archives out of the folder tree. The tree
        // cannot show one anyway: it builds nodes from ListDirectory filtered on IsDirectory, and
        // an archive is a file. A structural fact is a better guarantee than a method that reports
        // "no subfolders" about something it can list the subfolders of.
        if (Resolve(path) is not { } archive) return _inner.ProbeSubdirectories(path);

        var index = ReadArchive(archive.ArchiveFile);
        if (!index.Ok) return default;

        var children = index.Children(archive.EntryPath);
        if (children is null) return default;

        var any = children.Any(c => c.IsDirectory);
        // Nothing inside an archive is hidden, so "any" and "any visible" are the same answer.
        return new SubdirectoryPresence(any, any);
    }

    public IReadOnlyList<DriveInfo> GetDrives() => _inner.GetDrives();

    /// <summary>
    /// One entry as a listing row.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Nothing inside an archive is ever <c>Hidden</c>.</b> <c>IEntry.Attrib</c> is an
    /// <c>int?</c> holding either a DOS attribute byte or a Unix mode in its high sixteen bits,
    /// depending on which tool wrote the container — and "Show hidden items" filters on this. Map
    /// it blindly and most of a Linux tarball vanishes from the listing under the default setting.
    /// The payoff for getting it right is a ghosted icon; the cost of getting it wrong is files
    /// silently missing. So a dotfile shows, and that is the whole rule.
    /// </para>
    /// <para>
    /// A directory carries its exact recursive size rather than <c>-1</c>, which is what tells
    /// <c>FileItemViewModel</c> not to go asking <c>dir_size_cache</c> about a path that is not
    /// real. A null timestamp becomes <c>default</c>, which the Modified column renders blank.
    /// </para>
    /// </remarks>
    private static FileEntry ToEntry(string archiveFile, ArchiveNode node) => new(
        node.Name,
        ArchivePath.Compose(archiveFile, node.Path),
        node.IsDirectory,
        node.SizeBytes,
        node.Modified?.ToUniversalTime() ?? default,
        node.IsDirectory ? FileAttributes.Directory : FileAttributes.Normal);

    private static bool IsArchiveFile(string candidate)
    {
        try
        {
            return File.Exists(candidate);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                       or ArgumentException or NotSupportedException)
        {
            return false;
        }
    }
}
