namespace BertBrowser.Core.Services;

/// <summary>
/// Enough about an entry on disk to tell whether it is still the one something wrote.
/// </summary>
/// <remarks>
/// <para>
/// The undo history needs it where an undo would otherwise act on whatever happens to be at a path
/// now rather than on what it put there: removing a pasted file, and swapping an edited archive for
/// its original. A stamp that no longer matches means somebody else has been at the entry since,
/// and the answer is to leave it alone and say so.
/// </para>
/// <para>
/// <b>Creation time is part of it.</b> A copy keeps its source's last-write time, so an original
/// restored under a copy's name can match on length and last write — and a creation time is set
/// when the entry is made. Not infallibly: NTFS tunnels a creation time onto a name reused within
/// seconds, which is exactly what an Overwrite does. So the stamp is a second guard, not the only
/// one — the undo history never hands the same record to an undo twice.
/// </para>
/// </remarks>
public readonly record struct EntryStamp(long Length, DateTime WriteUtc, DateTime CreatedUtc)
{
    /// <summary>The stamp of whatever is at <paramref name="path"/>, or null when nothing is.
    /// A folder's length is -1: its contents are not measured, only its identity.</summary>
    public static EntryStamp? Of(string path)
    {
        try
        {
            var file = new FileInfo(path);
            if (file.Exists)
                return new EntryStamp(file.Length, file.LastWriteTimeUtc, file.CreationTimeUtc);

            var directory = new DirectoryInfo(path);
            if (directory.Exists)
                return new EntryStamp(-1, directory.LastWriteTimeUtc, directory.CreationTimeUtc);

            return null;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                       or ArgumentException or NotSupportedException)
        {
            return null;
        }
    }

    /// <summary>Whether what is at <paramref name="path"/> is still the entry this stamp was taken
    /// of. A folder is compared by creation time alone, since writing inside it moves its last-write
    /// time without making it a different folder.</summary>
    public bool Matches(string path) =>
        Of(path) is { } now &&
        now.Length == Length &&
        now.CreatedUtc == CreatedUtc &&
        (Length < 0 || now.WriteUtc == WriteUtc);
}
