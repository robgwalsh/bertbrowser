using System.Windows.Threading;
using BertBrowser.Harness;

// The harness runs the browser on its own STA thread with its own dispatcher, rather than through
// Application.Run, so that the script can drive the window from the outside and the process still
// ends when the script does.

var output = Console.Out;

var options = HarnessOptions.Parse(args, output, out var parseExit);
if (options is null) return parseExit;

// Nothing here has a window the user could find and close, so a run that wedged would sit in the
// background forever. The watchdog is what makes an invisible process safe to start.
using var watchdog = new Timer(
    _ =>
    {
        output.WriteLine($"FAIL watchdog — nothing finished within {options.TimeoutSeconds}s.");
        output.Flush();
        Environment.Exit(HarnessOptions.Exit.Timeout);
    },
    null,
    TimeSpan.FromSeconds(options.TimeoutSeconds),
    Timeout.InfiniteTimeSpan);

Directory.CreateDirectory(options.OutputDir);
output.WriteLine($"# out:     {options.OutputDir}");
output.WriteLine($"# sandbox: {options.SandboxDir}");
output.WriteLine($"# state:   {options.StateDir}");

var exitCode = HarnessOptions.Exit.Environment;

var thread = new Thread(() =>
{
    UiSession? session = null;

    try
    {
        session = UiSession.Start(options, output);
        exitCode = new ScriptRunner(session, options, output).Run();
    }
    catch (Exception e)
    {
        output.WriteLine($"FAIL harness — {e.Message}");
        exitCode = HarnessOptions.Exit.Environment;
    }
    finally
    {
        session?.Dispose();
        Dispatcher.CurrentDispatcher.InvokeShutdown();
    }
})
{
    IsBackground = true,
};

thread.SetApartmentState(ApartmentState.STA);
thread.Start();
thread.Join();

if (!options.KeepState) RemoveScratchState();

// The database keeps a WAL beside it and its connections are pooled, and the index watcher patches
// it whenever a file under a watched root changes — which a script that moves and deletes things
// does constantly — so a write can still be in flight when the window closes. Retried for a few
// seconds rather than reported first time, and reported rather than thrown in the end: a leftover
// scratch directory under %TEMP% is not a failed run.
void RemoveScratchState()
{
    for (var attempt = 0; ; attempt++)
    {
        try
        {
            if (Directory.Exists(options.StateDir)) Directory.Delete(options.StateDir, recursive: true);
            return;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            if (attempt >= 29)
            {
                output.WriteLine($"# could not remove the scratch state directory: {e.Message}");
                return;
            }

            // A connection that was never disposed is only closed by its finalizer, and the pool
            // only lets go of the ones that have been returned to it — so both, in that order.
            GC.Collect();
            GC.WaitForPendingFinalizers();
            Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
            Thread.Sleep(200);
        }
    }
}

output.Flush();

// WPF leaves foreground threads of its own behind, and a harness that does not come back is worse
// than one that fails, so the exit is explicit rather than a return from Main.
Environment.Exit(exitCode);
return exitCode;
