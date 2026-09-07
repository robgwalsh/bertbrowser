using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.App.Services.Indexing;

/// <summary>
/// Asks Windows whether an index helper is running, by looking for the name one holds for its life.
/// </summary>
/// <remarks>
/// A medium-integrity process may open a high-integrity one's mutex for <c>SYNCHRONIZE</c>, since
/// that is not a write right and mandatory policy is no-write-up. See
/// <see cref="IndexerPresenceLock"/> for the whole argument, and for why the helper grants this
/// user exactly that and nothing else.
/// </remarks>
public sealed class WindowsIndexerPresence : IIndexerPresence
{
    public bool IsRunning
    {
        get
        {
            try
            {
                return IndexerPresenceLock.IsHeld(IndexEndpoint.CurrentUserSid());
            }
            catch (InvalidOperationException)
            {
                // No SID for this token. Nothing can be running that we could reach anyway.
                return false;
            }
        }
    }
}
