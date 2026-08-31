using System.IO;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Elevation;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Services.Elevation;

/// <summary>
/// Starts <c>BertBrowser.Elevator.exe</c> for one file operation.
/// </summary>
/// <remarks>
/// One helper per operation, and it exits when that operation is done — so unlike the index
/// launcher, which keeps one handle for the session, this keeps one per launch and lets it go. The
/// prompt itself is <see cref="ElevatedProcess"/>, shared with the index launcher.
/// </remarks>
public sealed class ElevatedFileOperationLauncher : IElevationLauncher, IDisposable
{
    private readonly string _helperPath;
    private readonly Lock _gate = new();
    private readonly Dictionary<int, SafeProcessHandle> _running = [];

    public ElevatedFileOperationLauncher()
        : this(Path.Combine(AppContext.BaseDirectory, "BertBrowser.Elevator.exe"))
    {
    }

    internal ElevatedFileOperationLauncher(string helperPath) => _helperPath = helperPath;

    /// <inheritdoc/>
    public bool CanElevate => ElevatedProcess.CanElevate;

    public ElevationLaunchResult Launch(string pipeName, int parentProcessId, string userSid)
    {
        var started = ElevatedProcess.Start(
            _helperPath,
            FormatArguments(pipeName, parentProcessId, userSid),
            "the helper that does this is missing");

        switch (started.Outcome)
        {
            case ElevatedStart.Declined:
                return ElevationLaunchResult.Declined;

            case ElevatedStart.Failed:
                return ElevationLaunchResult.Failed(started.Detail);
        }

        lock (_gate)
        {
            // A pid can be reused once the process behind it has gone, so an old entry under the
            // same number is stale by definition and never the one being waited on.
            if (_running.Remove(started.ProcessId, out var stale)) stale.Dispose();
            if (started.Handle is { } handle) _running[started.ProcessId] = handle;
        }

        return ElevationLaunchResult.Started(started.ProcessId);
    }

    public void WaitForExit(int processId, TimeSpan timeout)
    {
        SafeProcessHandle? handle;
        lock (_gate)
        {
            _running.Remove(processId, out handle);
        }

        try
        {
            ElevatedProcess.WaitForExit(handle, timeout);
        }
        finally
        {
            handle?.Dispose();
        }
    }

    /// <summary>
    /// Quoted for the pipe name, which is generated here; the other two are a number and this
    /// process's own SID. Nothing in this line comes from a file being browsed.
    /// </summary>
    private static string FormatArguments(string pipeName, int parentProcessId, string userSid) =>
        $"--pipe \"{pipeName}\" --parent-pid {parentProcessId} --user-sid \"{userSid}\"";

    public void Dispose()
    {
        lock (_gate)
        {
            foreach (var handle in _running.Values) handle.Dispose();
            _running.Clear();
        }
    }
}
