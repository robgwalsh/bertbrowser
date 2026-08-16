namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// One listening endpoint waiting for the elevated indexer to call back.
/// </summary>
/// <remarks>
/// <b>The app listens and the helper connects, not the other way round.</b> A named pipe created by
/// a high-integrity process carries a High mandatory label, and mandatory policy is no-write-up, so
/// a medium-integrity client could not write to it — the app would need to start labelling objects
/// to talk to its own helper. Creating the pipe on the medium side makes the helper's connection a
/// write-<em>down</em>, which is always permitted, and the whole question disappears.
/// </remarks>
public interface IIndexTransport : IDisposable
{
    /// <summary>What the helper should be told to connect to.</summary>
    string Endpoint { get; }

    /// <summary>
    /// Waits for the indexer to connect, and returns null if nobody did in time or the peer was not
    /// who we started.
    /// </summary>
    /// <param name="processId">
    /// The process the launcher started. A DACL proves the peer is this user; only this proves it
    /// is the process we launched.
    /// </param>
    Stream? Accept(int processId, TimeSpan timeout);
}

/// <summary>Makes a fresh endpoint per attempt, since a retry needs a new name and a new pipe.</summary>
public interface IIndexTransportFactory
{
    IIndexTransport Create();
}
