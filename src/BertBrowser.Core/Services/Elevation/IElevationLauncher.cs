namespace BertBrowser.Core.Services.Elevation;

/// <summary>How starting the elevated helper went.</summary>
public enum ElevationLaunch
{
    Started,

    /// <summary>The user answered the UAC prompt with No — <c>ERROR_CANCELLED</c>. Their choice,
    /// reported as such rather than as a failure.</summary>
    Declined,

    /// <summary>No prompt was raised because this account cannot elevate.</summary>
    NotAdministrator,

    Failed,
}

/// <param name="Detail">Phrased for the status bar.</param>
public readonly record struct ElevationLaunchResult(
    ElevationLaunch Outcome, int ProcessId = 0, string Detail = "")
{
    public static ElevationLaunchResult Started(int processId) => new(ElevationLaunch.Started, processId);
    public static readonly ElevationLaunchResult Declined = new(ElevationLaunch.Declined);
    public static readonly ElevationLaunchResult NotAdministrator = new(ElevationLaunch.NotAdministrator);
    public static ElevationLaunchResult Failed(string detail) => new(ElevationLaunch.Failed, 0, detail);
}

/// <summary>
/// Starting one short-lived elevated helper. The seam that keeps a UAC prompt out of a test and out
/// of a scripted harness run.
/// </summary>
public interface IElevationLauncher
{
    /// <summary>
    /// Whether this account could elevate if asked. Asked <em>before</em> the consent dialog is
    /// shown, never after: a standard user shown a shield gets a credential prompt for somebody
    /// else's password, which is a good deal worse than never being offered one.
    /// </summary>
    /// <remarks>Implementations must read the token's elevation type, never
    /// <c>IsInRole(Administrator)</c> — an administrator running normally holds a filtered token in
    /// which that role is deny-only, so the role check answers false for precisely the people who
    /// can elevate.</remarks>
    bool CanElevate { get; }

    /// <summary>Raises the prompt and starts the helper.</summary>
    ElevationLaunchResult Launch(string pipeName, int parentProcessId, string userSid);

    /// <summary>Waits for a started helper to go away. The app cannot terminate it — a medium
    /// process may not open a high one for that — so this is a wait and never a kill.</summary>
    void WaitForExit(int processId, TimeSpan timeout);
}

/// <summary>The pipe the helper calls back on. Created app-side, so the high-integrity helper's
/// connection is a write-down: mandatory policy forbids writing <em>up</em>, so a pipe the helper
/// created could not be written to by the app that owns it.</summary>
public interface IElevationTransport : IDisposable
{
    string Endpoint { get; }

    /// <summary>Waits for the process just launched, and only that one, to connect.</summary>
    Stream? Accept(int processId, TimeSpan timeout);
}

/// <summary>One transport per operation: this helper is one-shot, so its pipe is too.</summary>
public interface IElevationTransportFactory
{
    IElevationTransport Create();
}
