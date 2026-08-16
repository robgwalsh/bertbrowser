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

    public IndexHostLaunchResult Launch(string pipeName, int parentProcessId) =>
        IndexHostLaunchResult.Declined;

    public void WaitForExit(int processId, TimeSpan timeout) { }
}

/// <summary>A transport nobody ever connects to, since nothing was started.</summary>
internal sealed class NoIndexTransportFactory : IIndexTransportFactory
{
    public IIndexTransport Create() => new NoIndexTransport();

    private sealed class NoIndexTransport : IIndexTransport
    {
        public string Endpoint => "BertBrowser.Index.Harness";
        public Stream? Accept(int processId, TimeSpan timeout) => null;
        public void Dispose() { }
    }
}
