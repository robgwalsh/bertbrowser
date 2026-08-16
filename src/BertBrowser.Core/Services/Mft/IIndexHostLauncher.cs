namespace BertBrowser.Core.Services.Mft;

/// <summary>Why the elevated indexer is or is not running.</summary>
public enum IndexHostLaunch
{
    /// <summary>It started. The client should expect a connection.</summary>
    Started,

    /// <summary>The user was asked for administrator rights and said no.</summary>
    Declined,

    /// <summary>This account cannot elevate at all, so there was nothing worth asking.</summary>
    NotAdministrator,

    /// <summary>It could not be started, for a reason the user did not choose.</summary>
    Failed,
}

/// <summary>
/// The outcome of asking for the indexer, and the process if there is one.
/// </summary>
/// <param name="Outcome">What happened.</param>
/// <param name="ProcessId">
/// The elevated process, when one started. The client compares this against the process that
/// actually connects to its pipe — the DACL proves the peer is this user, and only this proves it
/// is the process we started rather than another of the user's own that raced to connect.
/// </param>
/// <param name="Detail">A short reason for <see cref="IndexHostLaunch.Failed"/>, for the log.</param>
public readonly record struct IndexHostLaunchResult(
    IndexHostLaunch Outcome,
    int ProcessId = 0,
    string Detail = "")
{
    public static IndexHostLaunchResult Started(int processId) =>
        new(IndexHostLaunch.Started, processId);

    public static readonly IndexHostLaunchResult Declined = new(IndexHostLaunch.Declined);

    public static readonly IndexHostLaunchResult NotAdministrator = new(IndexHostLaunch.NotAdministrator);

    public static IndexHostLaunchResult Failed(string detail) =>
        new(IndexHostLaunch.Failed, 0, detail);
}

/// <summary>
/// Starts the elevated indexer. Behind an interface so the client's state machine can be tested
/// against every outcome without a real UAC prompt — which is the one thing a test can never drive.
/// </summary>
public interface IIndexHostLauncher
{
    /// <summary>
    /// Starts the indexer, telling it which pipe to call back on and which process to expect on the
    /// other end of it.
    /// </summary>
    IndexHostLaunchResult Launch(string pipeName, int parentProcessId);

    /// <summary>
    /// False when this account could not elevate even if it wanted to. Checked <em>before</em>
    /// prompting: a standard user gets a credential prompt asking for someone else's password
    /// rather than a consent prompt, and firing that at somebody who never asked for it, at
    /// startup, is much worse than quietly having no index.
    /// </summary>
    bool CanElevate { get; }

    /// <summary>Waits for a launched indexer to exit, up to <paramref name="timeout"/>.</summary>
    void WaitForExit(int processId, TimeSpan timeout);
}
