using System.IO;
using BertBrowser.App.Services.Elevation;
using BertBrowser.Core.Services.Mft;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.App.Services.Indexing;

/// <summary>
/// Starts <c>BertBrowser.Indexer.exe</c> with an administrator token.
/// </summary>
/// <remarks>
/// <para>
/// The prompt, the handle and the elevation-type check all live in <see cref="ElevatedProcess"/>,
/// shared with the file-operation helper's launcher. What is left here is what is particular to the
/// index helper: where its executable is, what it is called in a status message, and the shape of
/// its arguments.
/// </para>
/// <para>
/// The helper is resolved beside this executable and never by name — it must not be found on
/// <c>PATH</c>, since what is being launched is a thing that gets an administrator token.
/// </para>
/// </remarks>
public sealed class ElevatedIndexHostLauncher : IIndexHostLauncher, IDisposable
{
    private readonly string _helperPath;
    private readonly Lock _gate = new();
    private SafeProcessHandle? _process;
    private int _processId;

    public ElevatedIndexHostLauncher()
        : this(Path.Combine(AppContext.BaseDirectory, "BertBrowser.Indexer.exe"))
    {
    }

    internal ElevatedIndexHostLauncher(string helperPath) => _helperPath = helperPath;

    /// <inheritdoc/>
    public bool CanElevate => ElevatedProcess.CanElevate;

    public IndexHostLaunchResult Launch()
    {
        var started = ElevatedProcess.Start(
            _helperPath, FormatArguments(), "the index helper is missing");

        switch (started.Outcome)
        {
            case ElevatedStart.Declined:
                return IndexHostLaunchResult.Declined;

            case ElevatedStart.Failed:
                return IndexHostLaunchResult.Failed(started.Detail);
        }

        lock (_gate)
        {
            _process?.Dispose();
            _process = started.Handle;
            _processId = started.ProcessId;
        }

        return IndexHostLaunchResult.Started(started.ProcessId);
    }

    public void WaitForExit(int processId, TimeSpan timeout)
    {
        SafeProcessHandle? handle;
        lock (_gate)
        {
            handle = _processId == processId ? _process : null;
        }

        ElevatedProcess.WaitForExit(handle, timeout);
    }

    /// <summary>
    /// The helper is told where the database is, and nothing else.
    /// </summary>
    /// <remarks>
    /// Quoted, because the data directory can contain spaces — it is under the user's profile.
    /// Nothing here comes from a file being browsed; the path is this app's own. The pipe name used
    /// to be here too and is not any more: the helper derives it from its own token, so there is no
    /// endpoint for a caller to choose.
    /// </remarks>
    private static string FormatArguments() =>
        $"--data-dir \"{AppPaths.DataDir.TrimEnd('\\')}\"";

    public void Dispose()
    {
        lock (_gate)
        {
            _process?.Dispose();
            _process = null;
        }
    }
}
