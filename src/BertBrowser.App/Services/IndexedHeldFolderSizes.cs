using BertBrowser.Core.Data;
using BertBrowser.Core.Services.UndoHistory;

namespace BertBrowser.App.Services;

/// <summary>
/// A held folder's size from <c>dir_size_cache</c>, by the path it had before it was set aside —
/// one primary-key lookup, the same answer a transfer's estimate takes.
/// </summary>
/// <remarks>
/// A missing or incomplete row is "unknown", never zero, and sends the estimator to walk the held
/// copy instead. It is only ever an estimate for the undo budget, never a size anybody is shown for
/// a folder.
/// </remarks>
internal sealed class IndexedHeldFolderSizes(DirSizeRepository repository) : IHeldFolderSizes
{
    public long? Of(string originalPath)
    {
        try
        {
            return repository.Get(originalPath) is { Incomplete: false } row ? row.SizeBytes : null;
        }
        catch (Exception ex) when (ex is ArgumentException or InvalidOperationException
                                       or Microsoft.Data.Sqlite.SqliteException)
        {
            return null;
        }
    }
}
