using BertBrowser.Core.Services.Elevation;

namespace BertBrowser.Harness;

/// <summary>
/// An <see cref="IElevationLauncher"/> that starts nothing.
/// </summary>
/// <remarks>
/// The file-operation helper is the second thing in this app that raises a UAC prompt, and a prompt
/// takes the secure desktop — which is the one thing parking the window offscreen cannot work
/// around. A scripted run gets this, and <c>UiSession</c> asserts it did rather than trusting the
/// registration to stay put.
/// </remarks>
internal sealed class RefusingElevationLauncher : IElevationLauncher
{
    /// <summary>True, deliberately: a run should exercise the path the machine it runs on would
    /// take, and the refusal below is what keeps it safe. Reporting false here would silently skip
    /// every offer instead.</summary>
    public bool CanElevate => true;

    public ElevationLaunchResult Launch(string pipeName, int parentProcessId, string userSid) =>
        ElevationLaunchResult.Failed("the harness never starts the elevated helper");

    public void WaitForExit(int processId, TimeSpan timeout)
    {
    }
}

/// <summary>
/// An <see cref="IElevationPrompt"/> that answers however the script said, and remembers what it was
/// asked.
/// </summary>
/// <remarks>
/// Recording is the point: <c>assert-elevation-offered</c> is the only way to prove, end to end and
/// through the real executors, that a genuine access-denied failure reached the offer. No unit test
/// covers that seam, because the discriminator and the dialog live on opposite sides of it.
/// </remarks>
internal sealed class RecordingElevationPrompt : IElevationPrompt
{
    private readonly List<ElevationOffer> _offers = [];

    internal bool Answer { get; set; }

    internal IReadOnlyList<ElevationOffer> Offers => _offers;

    public bool Offer(ElevationOffer offer)
    {
        lock (_offers) _offers.Add(offer);
        return Answer;
    }
}
