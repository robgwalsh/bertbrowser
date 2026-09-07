namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// One listening endpoint waiting for the elevated indexer to call back.
/// </summary>
/// <remarks>
/// <b>The app listens and the helper connects, not the other way round.</b> A named pipe created by
/// a high-integrity process carries a High mandatory label, and mandatory policy is no-write-up, so
/// a medium-integrity client could not write to it — the app would need to start labelling objects
/// to talk to its own helper. Creating the pipe on the medium side makes the helper's connection a
/// write-<em>down</em>, which is always permitted, and the whole question disappears. It is also
/// why a helper that outlives the app is still not a server: nothing may connect <em>to</em> it.
/// </remarks>
public interface IIndexTransport : IDisposable
{
    /// <summary>What the helper should be told to connect to.</summary>
    string Endpoint { get; }

    /// <summary>
    /// Waits for an indexer to connect, and returns null if nobody did in time or the peer was not
    /// acceptable. Safe to call again after a timeout — the endpoint stays up, so a helper that
    /// starts later can still be picked up.
    /// </summary>
    /// <param name="launchedProcessId">
    /// The process this app started, when it started one. A DACL proves the peer is this user; this
    /// proves it is the process we launched. Null when <em>attaching</em> to a helper that was
    /// already running, where there is no such id and the peer's integrity level is checked instead.
    /// </param>
    Stream? Accept(int? launchedProcessId, TimeSpan timeout);
}

/// <summary>
/// Hands out the app's one endpoint.
/// </summary>
/// <remarks>
/// One for the app's life, not one per attempt. The name is well-known now — a helper started under
/// a previous app has to be able to find this one — so there is only one endpoint to have, and it
/// must stay up between attempts or a helper starting later would find nothing.
/// </remarks>
public interface IIndexTransportFactory
{
    /// <summary>
    /// The endpoint, or false and a short reason when it cannot be had — which for a per-user name
    /// means another copy of this app already owns it.
    /// </summary>
    bool TryCreate(out IIndexTransport? transport, out string error);
}
