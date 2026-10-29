using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Harness;

/// <summary>
/// An index-helper launcher that starts nothing and reports that the user declined.
/// </summary>
/// <remarks>
/// Behind <c>--index-declined</c>, so a script can photograph the degraded status bar. Only the
/// launcher is faked — the <see cref="MftIndexClient"/> above it is the real one, so what a run
/// sees is the genuine failure path and not a picture staged to look like it. Whether a person
/// says no to a UAC prompt is the one thing a scripted run can never decide for itself.
/// </remarks>
internal sealed class DecliningIndexHostLauncher : IIndexHostLauncher
{
    public bool CanElevate => true;

    public IndexHostLaunchResult Launch() => IndexHostLaunchResult.Declined;

    public void WaitForExit(int processId, TimeSpan timeout) { }
}

/// <summary>
/// A transport nobody ever connects to.
/// </summary>
/// <remarks>
/// It answers null to the attach look as well as the one after a launch, so a scripted run sees
/// exactly what a machine with no index helper sees — which is now the banner, and after clicking
/// through it, the declined prompt.
/// </remarks>
internal sealed class NoIndexTransportFactory : IIndexTransportFactory
{
    public bool TryCreate(out IIndexTransport? transport, out string error)
    {
        transport = new NoIndexTransport();
        error = "";
        return true;
    }

    private sealed class NoIndexTransport : IIndexTransport
    {
        public string Endpoint => "BertBrowser.Index.Harness";
        public Stream? Accept(int? launchedProcessId, TimeSpan timeout) => null;
        public void Dispose() { }
    }
}
