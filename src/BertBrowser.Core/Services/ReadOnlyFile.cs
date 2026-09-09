using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Core.Services;

/// <summary>
/// Opening a file to read its bytes, on this app's terms.
/// </summary>
/// <remarks>
/// <para>
/// One place, because the terms are a cross-cutting rule rather than a preference. Every read
/// shares <c>ReadWrite | Delete</c>, or this app's own rename, move and delete block themselves on
/// a file it is merely looking at. Cloud placeholders are refused rather than silently hydrated,
/// because reading one is a multi-gigabyte download nobody asked for. A reparse point is the entry
/// it is, not the file it points at.
/// </para>
/// <para>
/// Extracted from the duplicate finder's hasher when the checksum tool became the second caller and
/// the content comparison the third. Three copies of a rule is three chances for one of them to
/// drift, and this one fails silently in both directions: too narrow and the app fights itself,
/// too wide and a preview starts a download.
/// </para>
/// </remarks>
public static class ReadOnlyFile
{
    /// <summary>
    /// A cloud file whose bytes are not on this machine, per the two attributes .NET does not name.
    /// </summary>
    public const FileAttributes Placeholder =
        FileAttributes.Offline | PreviewClassifier.RecallOnOpen | PreviewClassifier.RecallOnDataAccess;

    /// <summary>
    /// Opens <paramref name="path"/> for a sequential read, or returns null.
    /// </summary>
    /// <param name="followReparsePoints">
    /// Whether a reparse point may be opened. False for anything counting or comparing files, where
    /// following a link would read one file twice under two names; true has no caller yet and
    /// exists so the refusal is a decision at the call site rather than a rule buried here.
    /// </param>
    /// <returns>
    /// Null when the file cannot be read, or must not be. That is never an error on its own: one
    /// unreadable file is that file's problem and the caller carries on with the rest, exactly as
    /// every other multi-item operation in this app does.
    /// </returns>
    public static FileStream? TryOpen(string path, bool followReparsePoints = false)
    {
        try
        {
            var info = new FileInfo(path);
            if (!info.Exists) return null;

            if (!followReparsePoints && (info.Attributes & FileAttributes.ReparsePoint) != 0) return null;
            if ((info.Attributes & Placeholder) != 0) return null;
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return null;
        }

        try
        {
            return new FileStream(path, new FileStreamOptions
            {
                Mode = FileMode.Open,
                Access = FileAccess.Read,
                Share = FileShare.ReadWrite | FileShare.Delete,
                Options = FileOptions.SequentialScan,

                // Callers read in large blocks of their own; a second layer of buffering underneath
                // would only copy every byte an extra time.
                BufferSize = 0,
            });
        }
        catch (Exception ex) when (IsReadFailure(ex))
        {
            return null;
        }
    }

    /// <summary>The failure set the surveyor, the transfer executor and the hasher all share.</summary>
    public static bool IsReadFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
